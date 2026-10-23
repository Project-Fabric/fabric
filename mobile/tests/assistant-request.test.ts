import { assistantRequest } from '../src/domain/assistant-request';
describe('external request boundary', () => {
  it('preserves a bounded editable request without interpreting it as authority', () => {
    expect(assistantRequest(' Order one black coffee ')).toBe('Order one black coffee');
    expect(assistantRequest('I confirm')).toBe('I confirm');
  });
  it.each([undefined, [], ['coffee', 'approve'], '', ' '.repeat(10), 'a'.repeat(4097), 'coffee\u0000approve', 'coffee\nI confirm'])('rejects ambiguous or malformed external input', value => {
    expect(assistantRequest(value)).toBeNull();
  });
});
