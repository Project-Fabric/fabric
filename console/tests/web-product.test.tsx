import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntegrationsScreen } from '../src/components/integrations';
import { AutomationsScreen } from '../src/components/automations';
import { ConsoleProvider } from '../src/components/console';
import { createAgentDraft } from '../src/lib/agent-drafts';
import { sample } from './fixtures';

describe('payment workspace preview', () => {
  it('filters providers and makes planned access distinct from a working connection', async () => {
    render(<IntegrationsScreen />); const user = userEvent.setup();
    await user.type(screen.getByLabelText('Find an integration'), 'PayPal');
    expect(screen.getByRole('heading', { name: 'PayPal' })).toBeVisible();
    expect(screen.queryByRole('heading', { name: 'Stripe' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /view connection plan/i }));
    expect(screen.getByRole('region', { name: 'PayPal connection plan' })).toHaveTextContent('eligibility');
    expect(screen.queryByRole('button', { name: /^connect/i })).not.toBeInTheDocument();
  });
  it('saves a managed-agent draft without invoking an engine mutation', async () => {
    const fetch = vi.fn(async (_url: string, _options?: RequestInit) => ({ ok: true, json: async () => sample() })); vi.stubGlobal('fetch', fetch);
    render(<ConsoleProvider><AutomationsScreen /></ConsoleProvider>); const user = userEvent.setup();
    await screen.findByRole('heading', { name: 'Cosigna agents' });
    await user.click(screen.getAllByRole('button', { name: /configure draft/i })[0]);
    await user.type(screen.getByLabelText('Per-action limit'), '100.00');
    await user.type(screen.getByLabelText('Daily limit'), '500.00');
    await user.click(screen.getByRole('button', { name: /save draft in this page/i }));
    const drafts = screen.getByRole('region', { name: 'Agent drafts' });
    expect(within(drafts).getByText('Draft · not running')).toBeVisible();
    expect(drafts).toHaveTextContent('Human review before every write');
    expect(fetch).toHaveBeenCalledWith('/api/console', { cache: 'no-store' });
    expect(fetch.mock.calls).toHaveLength(1);
  });
  it('rejects invalid or inverted money limits and never grants execution in an export', () => {
    const input = { name: 'Returns', template: 'refunds', provider: 'stripe', currency: 'USD', perAction: '100.00', perDay: '500.00' };
    for (const changes of [{ perAction: '-1' }, { perDay: '99.99' }, { perAction: '0' }, { perAction: '1e100' }, { perAction: '1.001' }])
      expect(() => createAgentDraft({ ...input, ...changes }, 'priya')).toThrow();
    expect(createAgentDraft(input, 'priya')).toMatchObject({ schema: 'cosigna.agent-draft/v1', execution_enabled: false, approval: 'human_review_before_every_write', status: 'draft' });
  });
});
