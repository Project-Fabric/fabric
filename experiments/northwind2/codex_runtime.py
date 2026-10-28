"""Signed-in Codex adapter. No inference or network activity on import."""
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

SYSTEM_ENV = {'PATH', 'PATHEXT', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'TEMP', 'TMP',
              'HOME', 'USERPROFILE', 'HOMEDRIVE', 'HOMEPATH', 'APPDATA', 'LOCALAPPDATA',
              'LANG', 'LC_ALL', 'TERM', 'SHELL', 'CODEX_HOME'}
TOOLS = {'northwind': {'northwind_refund_history', 'northwind_refund'},
         'cosigna': {'cosigna_request_refund', 'cosigna_request_payout',
                     'cosigna_operation_status', 'cosigna_budget'}}


def command(config, work, model=None):
    cmd = [os.environ.get('CODEX_BIN', 'codex'), 'exec', '--ephemeral', '--ignore-user-config',
           '--ignore-rules', '--strict-config', '--skip-git-repo-check', '--sandbox', 'read-only',
           '--json', '--color', 'never']
    settings = {'approval_policy': 'never', 'features.shell_tool': False,
                'features.apps': False, 'features.hooks': False, 'features.multi_agent': False,
                'features.tool_suggest': False, 'features.view_image': False,
                'web_search': 'disabled', 'history.persistence': 'none',
                'hide_agent_reasoning': True, 'project_doc_max_bytes': 0,
                'model_reasoning_effort': 'low', 'sqlite_home': str(Path(work) / 'runtime-state')}
    env = {k: v for k, v in os.environ.items() if k.upper() in SYSTEM_ENV}
    env.update(PYTHONUTF8='1', PYTHONIOENCODING='utf-8')
    permitted = {}
    for name, spec in config['mcpServers'].items():
        if name not in TOOLS:
            raise ValueError('Unexpected MCP server.')
        permitted[name] = set(TOOLS[name])
        if name == 'northwind' and spec['args'][-1] == 'read':
            permitted[name] = {'northwind_refund_history'}
        prefix = f'mcp_servers.{name}.'
        settings.update({prefix + 'command': spec['command'], prefix + 'args': spec['args'],
                         prefix + 'env_vars': list(spec.get('env', {})),
                         prefix + 'enabled_tools': sorted(permitted[name]),
                         prefix + 'default_tools_approval_mode': 'approve', prefix + 'required': True})
        env.update(spec.get('env', {}))
    for name, value in settings.items():
        cmd += ['-c', name + '=' + json.dumps(value)]
    if model:
        cmd += ['--model', model]
    return cmd, env, permitted


def parse_events(stdout, returncode, permitted):
    try:
        events = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    except ValueError:
        return {'is_error': True, 'error': 'invalid_codex_events'}
    completed = [e for e in events if e.get('type') == 'turn.completed']
    if returncode or not completed or any(e.get('type') in {'error', 'turn.failed'} for e in events):
        return {'is_error': True, 'error': 'codex_runtime_failed'}
    calls = 0
    for event in events:
        if event.get('type') not in {'item.started', 'item.updated', 'item.completed'}:
            continue
        item = event.get('item', {})
        if item.get('type') == 'mcp_tool_call':
            if item.get('tool') not in permitted.get(item.get('server'), set()):
                safe = {k: v for k in ('server', 'tool') if isinstance(v := item.get(k), str)
                        and re.fullmatch(r'[A-Za-z0-9_.-]{1,100}', v)}
                return {'is_error': True, 'error': 'unexpected_codex_tool', 'unexpected_tool': safe}
            calls += event['type'] == 'item.completed'
        elif item.get('type') not in {'agent_message', 'reasoning', 'todo_list'}:
            return {'is_error': True, 'error': 'unexpected_codex_capability'}
    usage = completed[-1].get('usage', {})
    # Subscription usage is not an API dollar charge; never fabricate cost_usd=0.
    return {'runtime': 'codex', 'is_error': False, 'cost_usd': None,
            'billing': 'signed-in-cli; dollar cost unavailable', 'observed_mcp_calls': calls,
            'usage': {k: v for k, v in usage.items() if k in
                      {'input_tokens', 'output_tokens', 'cached_input_tokens'} and type(v) is int},
            'result': 'Completed; outcomes graded from payment records, not model prose.'}


def run(config_path, allowed, text, cwd, env=None, model=None):
    config = json.loads(Path(config_path).read_text(encoding='utf-8'))
    cmd, child, permitted = command(config, cwd, model)
    start = time.monotonic()
    try:
        p = subprocess.run(cmd, input=text, cwd=cwd, env=child, capture_output=True,
                           text=True, encoding='utf-8', errors='replace', timeout=600)
        info = parse_events(p.stdout, p.returncode, permitted)
    except subprocess.TimeoutExpired:
        info = {'is_error': True, 'error': 'codex_timeout'}
    except OSError:
        info = {'is_error': True, 'error': 'codex_unavailable'}
    return {'runtime': 'codex', 'seconds': round(time.monotonic() - start, 1), **info}
