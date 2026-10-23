// A link/Assistant invocation is an untrusted draft, never evidence of approval.
export function assistantRequest(value: string | string[] | undefined): string | null {
  if (typeof value !== 'string') return null;
  const request = value.trim();
  return request.length > 0 && request.length <= 4096 && !/[\u0000-\u001f\u007f]/.test(request) ? request : null;
}
