import '@testing-library/jest-dom/vitest';
import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
vi.mock('next/navigation', () => ({ usePathname: () => '/approvals' }));
if (!globalThis.ResizeObserver) globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
