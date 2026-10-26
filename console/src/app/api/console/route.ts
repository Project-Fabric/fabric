import { NextResponse } from 'next/server';
import { requireRequest } from '../../../../server/security.mjs';
import { snapshot, perform, timeline } from '@/lib/engine';
import { actionSchema } from '@/lib/contracts';
import { redactText } from '@/lib/redact';
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';
export async function GET(request: Request) {
  let session;
  try { session = requireRequest(request); }
  catch { return NextResponse.json({ error: 'Session unavailable. Open the access URL from your terminal.' }, { status: 401 }); }
  const id = new URL(request.url).searchParams.get('timeline');
  try {
    return NextResponse.json(id ? await timeline(session, id) : await snapshot(session), { headers: { 'Cache-Control': 'no-store' } });
  } catch (error) {
    return NextResponse.json({ error: redactText(error instanceof Error ? error.message : 'Could not load this.') }, { status: 409 });
  }
}
export async function POST(request: Request) {
  let session;
  try { session = requireRequest(request, true); }
  catch { return NextResponse.json({ error: 'Session or CSRF check failed.' }, { status: 403 }); }
  try {
    const raw = await request.text();
    if (Buffer.byteLength(raw) > 65536) return NextResponse.json({ error: 'Request too large.' }, { status: 413 });
    const action = actionSchema.safeParse(JSON.parse(raw));
    if (!action.success) return NextResponse.json({ error: 'Invalid action fields. Nothing was submitted to the engine.' }, { status: 400 });
    return NextResponse.json(await perform(action.data, session), { headers: { 'Cache-Control': 'no-store' } });
  } catch (error) {
    return NextResponse.json({ error: redactText(error instanceof Error ? error.message : 'The engine refused this action.') }, { status: 409 });
  }
}
