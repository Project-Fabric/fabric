import { LabApiError, LabClient, labUrl, minorUnits, quoteApprovable, type LabTaskResult } from '../src/domain/lab';

describe('connected lab contract', () => {
  it('allows only the default loopback backend or an explicit private Wi-Fi relay', () => {
    expect(labUrl()).toBe('http://127.0.0.1:3097');
    for (const host of ['192.168.0.181', '10.0.0.2', '172.16.0.5', '172.31.10.8']) expect(labUrl(`http://${host}:3099`)).toBe(`http://${host}:3099`);
    for (const value of ['http://8.8.8.8:3099', 'http://172.32.0.2:3099', 'http://192.168.300.1:3099', 'http://192.168.001.2:3099', 'http://192.168.0.2:3097', 'http://user:secret@192.168.0.2:3099', 'http://192.168.0.2:3099/path', 'http://192.168.0.2:3099?token=x', 'https://untrusted.example']) {
      expect(() => labUrl(value)).toThrow('private Wi-Fi sandbox');
    }
  });
  it('parses minor units exactly and rejects overflow, negative and fractional cents', () => {
    expect(minorUnits('7.01')).toBe(701);
    for (const value of ['0', '-1', '1.001', '1e3', '1000.01', 'NaN']) expect(() => minorUnits(value)).toThrow();
  });
  it('rejects expired and uncertain quotes before exposing approval', () => {
    const result = { approval_digest: 'a'.repeat(64), data: { stage: 'quoted', quote: { total_minor: 700, expires_at: 200, offer: { currency: 'USD' } } } } as LabTaskResult;
    expect(quoteApprovable(result, 199_000)).toBe(true);
    expect(quoteApprovable(result, 200_000)).toBe(false);
    result.data.stage = 'unknown'; expect(quoteApprovable(result, 100_000)).toBe(false);
    result.data.stage = 'quoted'; result.approval_digest = 'invalid'; expect(quoteApprovable(result, 100_000)).toBe(false);
  });
  it('pins the USB endpoint, adds explicit consent, refuses redirects and validates response schema', async () => {
    const transport = jest.fn().mockResolvedValue({ ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { ready: true } }) });
    const client = new LabClient('synthetic-token', transport);
    await expect(client.call('/plan', { request: 'Order one oat latte' }, 'planner-v1')).resolves.toEqual({ ready: true });
    expect(transport).toHaveBeenCalledWith('http://127.0.0.1:3097/mobile/v3/plan', expect.objectContaining({ redirect: 'error',
      headers: expect.objectContaining({ Authorization: 'Bearer synthetic-token', 'X-Cosigna-Hosted-Consent': 'planner-v1' }) }));
    await expect(client.call('//untrusted.example')).rejects.toThrow('Invalid lab endpoint');
    transport.mockResolvedValue({ ok: true, status: 200, json: async () => ({ schema: 'different/v1', data: {} }) });
    await expect(client.call('/state')).rejects.toBeInstanceOf(LabApiError);
  });
  it('never automatically retries an ambiguous submission', async () => {
    const transport = jest.fn().mockRejectedValue(new Error('synthetic lost response'));
    const client = new LabClient('synthetic-token', transport);
    await expect(client.call('/approve', { task_id: 'synthetic' })).rejects.toMatchObject({ code: 'connection_lost' });
    expect(transport).toHaveBeenCalledTimes(1);
  });
});
