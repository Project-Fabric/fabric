"""MCP stdio relay recording only bounded financial fields, not prose or credentials."""
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from demo import child_env, safe_trace_call, safe_trace_result


def main():
    binary, url, trace = sys.argv[1:]
    token = os.environ.get('COSIGNA_AGENT_TOKEN', '')
    process = subprocess.Popen([binary, 'mcp', '--gateway-url', url],
                               env=child_env({'COSIGNA_AGENT_TOKEN': token}),
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               encoding='utf-8', bufsize=1)
    pending = {}
    calls = 0
    lock = threading.Lock()

    def receive():
        for line in process.stdout:
            try:
                data = json.loads(line)
                with lock:
                    call = pending.pop(str(data.get('id')), None)
                if call:
                    event = {'at': int(time.time()), **call,
                             'result': safe_trace_result(data.get('result', {}))}
                    with Path(trace).open('a', encoding='utf-8') as out:
                        out.write(json.dumps(event) + '\n')
            except (ValueError, TypeError):
                pass
            sys.stdout.write(line)
            sys.stdout.flush()

    thread = threading.Thread(target=receive, daemon=True)
    thread.start()
    try:
        for line in sys.stdin:
            if len(line.encode()) > 65536:
                break
            try:
                data = json.loads(line)
                if data.get('method') == 'tools/call':
                    calls += 1
                    if calls > 12:
                        break
                    with lock:
                        pending[str(data.get('id'))] = safe_trace_call(data.get('params', {}))
            except ValueError:
                pass
            process.stdin.write(line)
            process.stdin.flush()
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        thread.join(timeout=2)
        process.stdout.close()


if __name__ == '__main__':
    main()
