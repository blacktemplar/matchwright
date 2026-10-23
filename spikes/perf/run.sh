#!/bin/bash
# Times both spikes in headless Chrome (needs chromedriver), unthrottled and with the CPU slowed
# down 4x, Lighthouse's mid-range phone. Builds them first.
set -eu
cd "$(dirname "$0")/.."
mkdir -p target/perf
for spike in spike-leptos spike-dioxus; do
  (cd $spike && trunk build --release -d "../target/perf/$spike" >/dev/null)
done
(cd target/perf/spike-leptos && exec python3 -m http.server 8471 >/dev/null 2>&1) &
L=$!
(cd target/perf/spike-dioxus && exec python3 -m http.server 8472 >/dev/null 2>&1) &
D=$!
chromedriver --port=9515 >/dev/null 2>&1 &
C=$!
trap 'kill $L $D $C' EXIT
sleep 1.5
S=$(curl -s -X POST localhost:9515/session -d '{"capabilities":{"alwaysMatch":{"goog:chromeOptions":{"args":["--headless=new","--window-size=1280,900"]}}}}' | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["value"]["sessionId"])')
curl -s -X POST localhost:9515/session/$S/timeouts -d '{"script":600000}' >/dev/null
P=$(python3 -I -c 'import json,sys;print(json.dumps({"script":open(sys.argv[1]).read(),"args":[]}))' perf/perf.js)
for rate in 1 4; do
  for port in 8471 8472; do
    name=$([ $port = 8471 ] && echo Leptos || echo Dioxus)
    curl -s -X POST localhost:9515/session/$S/goog/cdp/execute -d "{\"cmd\":\"Emulation.setCPUThrottlingRate\",\"params\":{\"rate\":$rate}}" >/dev/null
    curl -s -X POST localhost:9515/session/$S/url -d "{\"url\":\"http://localhost:$port/\"}" >/dev/null
    curl -s -X POST localhost:9515/session/$S/execute/async -d '{"script":"setTimeout(arguments[0], 3000)","args":[]}' >/dev/null
    echo "== $name, CPU slowed ${rate}x"
    curl -s -X POST localhost:9515/session/$S/execute/async -d "$P" | python3 -I -c 'import json,sys
for name, ms in json.load(sys.stdin)["value"]: print(f"  {name:<50} {ms:>7} ms")'
  done
done
curl -s -X DELETE localhost:9515/session/$S >/dev/null
