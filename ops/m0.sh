#!/bin/bash
# M0: 캐릭터 생성·아이템 등록 동안 1번 CPU 측정. 사용: bash ~/m0.sh <이름> (예: character, item-image, item-bake)
set -u
LABEL=${1:-run}
OUT=~/m0-$LABEL-$(date +%m%d-%H%M%S); mkdir -p "$OUT"; cd "$OUT" || exit 1
HZ=$(getconf CLK_TCK)

PYL=$(pgrep -d, -f uvicorn); PY=${PYL//,/ }
JAVA=$(pgrep -x java | tr '\n' ' ')
[ -z "$PY" ] && { echo "uvicorn 프로세스 없음: ps -ef | grep -i python 결과를 보여주세요"; exit 1; }
echo "== Python 프로세스"; ps -o pid,ppid,etime,rss,cmd -p "$PYL" | cut -c1-160
ROOT=$(for p in $PY; do pp=$(ps -o ppid= -p "$p" | tr -d ' '); case " $PY " in *" $pp "*) ;; *) echo "$p";; esac; done | head -1)
# 워커 자식 프로세스(multiprocessing spawn)는 cmdline에 uvicorn이 없음 : ROOT의 자손을 모두 더한다
desc() { local c; for c in $(pgrep -P "$1"); do echo "$c"; desc "$c"; done; }
PY="$ROOT $(desc "$ROOT" | tr '\n' ' ')"
echo "== 측정 기준 PID: $ROOT, 자손 포함 Python PID: $PY, vCPU: $(nproc)"
echo "== 시작 메모리"; free -m

sudo -v || exit 1
PYSPY=$(command -v py-spy || ls ~/.local/bin/py-spy 2>/dev/null)
if [ -z "$PYSPY" ]; then
  echo "== py-spy 설치 중"; python3 -m pip install --user -q py-spy && PYSPY=~/.local/bin/py-spy
fi
[ -x "${PYSPY:-}" ] || { echo "!! py-spy 설치 실패: 비중 분석 없이 CPU만 잰다"; PYSPY=""; }

ticks() { local s=0 p; for p in $1; do [ -r "/proc/$p/stat" ] && s=$((s + $(awk '{print $14+$15}' "/proc/$p/stat"))); done; echo "$s"; }

vmstat -n 1 > vmstat.log &
VM=$!
# 5초마다 프로세스 이름별 RSS(KB) 합
while :; do ps -eo rss=,comm= | awk -v t="$(date +%s)" '{m[$2]+=$1} END{for(c in m) print t, c, m[c]}' >> mem.log; sleep 5; done &
MEM=$!
[ -n "$PYSPY" ] && { sudo "$PYSPY" record -p "$ROOT" --subprocesses --nonblocking -r 20 -f raw -o profile.txt > pyspy.log 2>&1 & }
UNIT=$(ps -o unit= -p "$ROOT" 2>/dev/null | tr -d ' ')
if [[ $UNIT == *.service ]]; then
  echo "== 로그 따라가기: $UNIT"
  sudo journalctl -u "$UNIT" -f -n 0 -o cat | grep --line-buffered -E '굽기|아이템|합성' | tee bake.log &
else
  echo "== systemd 서비스가 아님($UNIT): 로그의 '굽기 시간'은 직접 확인"
fi

T0=$(date +%s); P0=$(ticks "$PY"); J0=$(ticks "$JAVA")
echo
echo ">> [$LABEL] 측정 시작. 지금 브라우저에서 측정할 작업을 하세요."
echo ">> 작업이 끝나면 Enter (캐릭터: '전체 굽기 시간' 로그, 아이템 전체 굽기: 마지막 '전체 굽기 묶음' 로그·관리자 화면 완료)"
read -r
T1=$(date +%s); P1=$(ticks "$PY"); J1=$(ticks "$JAVA")

kill "$VM" "$MEM" 2>/dev/null
[ -n "$PYSPY" ] && { sudo pkill -INT -f "py-spy record -p $ROOT"; sleep 3; }
sudo pkill -f "journalctl -u $UNIT -f" 2>/dev/null

SEC=$((T1 - T0)); [ "$SEC" -lt 1 ] && SEC=1
{
echo "===== M0 결과 [$LABEL] ($(date '+%F %T'))"
echo "측정 시간: ${SEC}초, vCPU: $(nproc)"
awk -v d=$((P1 - P0)) -v j=$((J1 - J0)) -v hz="$HZ" -v s="$SEC" 'BEGIN{
  printf "Python CPU: %.1f초 (vCPU 1개 기준 평균 %.0f%%)\n", d/hz, d/hz/s*100
  printf "Java   CPU: %.1f초 (vCPU 1개 기준 평균 %.0f%%)\n", j/hz, j/hz/s*100 }'
awk 'NR==2{for(i=1;i<=NF;i++)h[$i]=i; next}
  $1 ~ /^[0-9]+$/ { if(++row==1) next; n++
    us+=$h["us"]; sy+=$h["sy"]; id+=$h["id"]; wa+=$h["wa"]; st+=$h["st"]
    b=100-$h["id"]; if(b>max)max=b; if(b>=95)full++
    if(min==""||$h["free"]<min)min=$h["free"]; if($h["swpd"]>sw)sw=$h["swpd"]; si+=$h["si"]; so+=$h["so"] }
  END{ if(!n){print "vmstat 기록 없음"; exit}
    printf "서버 전체(vmstat %d초): us %.0f%% sy %.0f%% id %.0f%% wa %.0f%% st %.0f%%\n", n, us/n, sy/n, id/n, wa/n, st/n
    printf "바쁨 최대 %d%%, 95%% 이상인 초: %d/%d\n", max, full, n
    printf "메모리: free 최소 %dKB, swap 최대 %dKB, swap in/out 합 %d/%d\n", min, sw, si, so }' vmstat.log
if [ -s mem.log ]; then
  echo "--- 프로세스별 RSS 최대 (MB, 상위 6)"
  awk '{if($3>m[$2])m[$2]=$3} END{for(c in m) printf "%6.0f  %s\n", m[c]/1024, c}' mem.log | sort -rn | head -6
fi
if [ -s profile.txt ]; then
  echo "--- py-spy 비중 (활성 샘플 기준)"
  awk '{c=$NF; t+=c
    if($0 ~ /numpy|scipy|PIL|character\/(bake|engine|compose|canvas)\.py/) a+=c
    else if($0 ~ /openai|httpx|httpcore|ssl|socket|json|base64/) o+=c
    else e+=c }
    END{ if(!t){print "샘플 없음"; exit}
      printf "이미지 계산(numpy·scipy·Pillow): %.0f%%\nOpenAI·HTTP·JSON: %.0f%%\n그 밖: %.0f%%\n샘플 수: %d\n", a/t*100, o/t*100, e/t*100, t }' profile.txt
  echo "--- 많이 쓴 함수 상위 12 (맨 끝 프레임)"
  awk '{c=$NF; s=substr($0,1,length($0)-length($NF)-1); k=split(s,f,";"); leaf[f[k]]+=c; t+=c}
    END{for(x in leaf) printf "%5.1f%%  %s\n", leaf[x]/t*100, x}' profile.txt | sort -rn | head -12 | cut -c1-150
else
  echo "--- py-spy 결과 없음 (pyspy.log 확인)"; tail -3 pyspy.log 2>/dev/null
fi
[ -s bake.log ] && { echo "--- 굽기 로그"; grep -v '전체' bake.log | grep -c '굽기 시간: pixel' | sed 's/^/아이템 굽기 건수: /'; grep '전체 굽기' bake.log; grep -E '실패' bake.log | head -5; }
echo "결과 폴더: $OUT"
} | tee summary.txt
