import { NextResponse } from 'next/server';
import { requireRequest } from '../../../../server/security.mjs';
import { snapshot, perform } from '@/lib/engine';
import { actionSchema } from '@/lib/contracts';
import { redactText } from '@/lib/redact';
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';
export async function GET(request: Request) {
  try { return NextResponse.json(await snapshot(requireRequest(request)), { headers: { 'Cache-Control': 'no-store' } }); }
  catch { return NextResponse.json({ error: 'Session unavailable. Open the access URL from your terminal.' }, { status: 401 }); }
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
