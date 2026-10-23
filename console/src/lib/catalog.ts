export type Integration = {
  id: string; name: string; mark: string; category: 'Payments' | 'Accounting' | 'Commerce';
  stage: 'Test-mode validated' | 'Planned'; description: string; access: string[];
  next: string; docs: string;
};

export const integrations: Integration[] = [
  { id: 'stripe', name: 'Stripe', mark: 'S', category: 'Payments', stage: 'Test-mode validated',
    description: 'Start with the refund workflow exercised in the Northwind test-mode experiment.',
    access: ['Read payment and refund records', 'Scope refund writes to approved operations', 'Verify provider outcomes before retrying'],
    next: 'Merchant authorization and a production connection flow are still to be built. Recorded Stripe activity is not proof of an active account connection.',
    docs: 'https://docs.stripe.com/connect' },
  { id: 'paypal', name: 'PayPal', mark: 'P', category: 'Payments', stage: 'Planned',
    description: 'A proposed adapter for payment records and controlled refunds.',
    access: ['Confirm the permitted account authorization model', 'Separate transaction reads from refund writes', 'Validate provider events and reconcile results'],
    next: 'Validate account and partner eligibility, then implement and test the adapter.', docs: 'https://developer.paypal.com/platforms/seller-onboarding' },
  { id: 'razorpay', name: 'Razorpay', mark: 'R', category: 'Payments', stage: 'Planned',
    description: 'A proposed connection for payment and refund operations in supported markets.',
    access: ['Establish technology-partner access where required', 'Authorize the merchant account', 'Limit agent actions to reviewed capabilities'],
    next: 'Confirm launch-market requirements and implement the approved account access flow.', docs: 'https://razorpay.com/docs/partners/technology-partners/onboard-businesses/integrate-oauth/' },
  { id: 'adyen', name: 'Adyen', mark: 'A', category: 'Payments', stage: 'Planned',
    description: 'A proposed adapter for payment operations and their recorded outcomes.',
    access: ['Confirm the account and platform integration model', 'Separate capabilities by permitted operation', 'Verify and reconcile event delivery'],
    next: 'Validate the intended account model before starting provider-specific implementation.', docs: 'https://docs.adyen.com/' },
  { id: 'square', name: 'Square', mark: '□', category: 'Payments', stage: 'Planned',
    description: 'A candidate connection for merchant payment workflows.',
    access: ['Research merchant authorization', 'Define read and write capabilities', 'Test event and retry behavior'],
    next: 'Discovery and adapter implementation have not started.', docs: 'https://developer.squareup.com/docs' },
  { id: 'quickbooks', name: 'QuickBooks', mark: 'qb', category: 'Accounting', stage: 'Planned',
    description: 'A candidate source for invoice and reconciliation context.',
    access: ['Research company-account authorization', 'Start with read-only invoice context', 'Require explicit approval for accounting writes'],
    next: 'Scope the accounting workflow before implementing a connector.', docs: 'https://developer.intuit.com/app/developer/qbo/docs/get-started' },
  { id: 'xero', name: 'Xero', mark: 'x', category: 'Accounting', stage: 'Planned',
    description: 'A candidate source for accounting and reconciliation context.',
    access: ['Research organization authorization', 'Start with read-only records', 'Keep ledger changes outside an initial read-only workflow'],
    next: 'Scope the accounting workflow before implementing a connector.', docs: 'https://developer.xero.com/documentation/' },
  { id: 'shopify', name: 'Shopify', mark: 's', category: 'Commerce', stage: 'Planned',
    description: 'A candidate source for orders and customer support context.',
    access: ['Research store authorization', 'Read only the order context a workflow needs', 'Keep order actions separate from payment-provider permissions'],
    next: 'Confirm the store and payment-provider boundary before implementation.', docs: 'https://shopify.dev/docs/apps' },
];

export const agentTemplates = [
  { id: 'refunds', title: 'Refunds agent', initials: 'RF', description: 'Review a refund request, check the transaction, and prepare an amount for approval.',
    tools: ['Read payments', 'Read refunds', 'Propose refund'], writes: true },
  { id: 'collections', title: 'Collections assistant', initials: 'CO', description: 'Review unpaid invoices and prepare follow-up drafts for your team.',
    tools: ['Read invoices', 'Prepare follow-up draft'], writes: false },
  { id: 'reconciliation', title: 'Reconciliation agent', initials: 'RE', description: 'Compare transactions and payout records, then flag differences for review.',
    tools: ['Read transactions', 'Read payouts', 'Prepare reconciliation'], writes: false },
] as const;
