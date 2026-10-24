import type { ConsoleAction } from './contracts';

/** Arguments only: execFile receives these verbatim. No shell, quoting or interpolation. */
export function buildCommand(action: ConsoleAction, user: string, exportPath?: string): string[] {
  switch (action.action) {
    case 'approve': return ['approve', String(action.id), '--yes', '--as', user,
      ...(action.decision === 'reject' ? ['--reject'] : []), ...(action.note ? ['--note', action.note] : []), '--json'];
    case 'agent-add': return ['agent', 'add', action.name, ...action.tools.flatMap(t => ['--tool', t]), '--json'];
    case 'agent-revoke': return ['agent', 'revoke', action.name, '--json'];
    case 'mandate-revoke': return ['mandate', 'revoke', String(action.id), '--json'];
    case 'mandate-grant': return ['mandate', 'grant', action.title, ...action.tools.flatMap(t => ['--tool', t]),
      ...(action.agent ? ['--agent', action.agent] : []), ...(action.maxPerCall ? ['--max-per-call', action.maxPerCall] : []),
      ...(action.maxPerDay ? ['--max-per-day', action.maxPerDay] : []),
      ...(action.maxCallsPerDay ? ['--max-calls-per-day', String(action.maxCallsPerDay)] : []),
      ...(action.expires ? ['--expires', action.expires] : []), '--json'];
    case 'operation-decide':
    case 'operation-resolve':
      throw new Error('Gateway decisions go to the Cosigna gateway, not the engine CLI.');
    case 'evidence-export':
      if (!exportPath) throw new Error('Configure CONSOLE_EXPORT_DIR before exporting.');
      return ['evidence', 'export', '--out', exportPath, '--json'];
  }
}
