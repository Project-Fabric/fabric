import { renderRouter, screen, fireEvent } from 'expo-router/testing-library';
import RootLayout from '../src/app/_layout';
import TabLayout from '../src/app/(tabs)/_layout';
import Home from '../src/app/(tabs)/index';
import Activity from '../src/app/(tabs)/activity';
import Controls from '../src/app/(tabs)/controls';
import Task from '../src/app/tasks/[id]';
import NotFound from '../src/app/+not-found';

const routes = { _layout: RootLayout, '(tabs)/_layout': TabLayout, '(tabs)/index': Home,
  '(tabs)/activity': Activity, '(tabs)/controls': Controls, 'tasks/[id]': Task, '+not-found': NotFound };
afterEach(() => { jest.useRealTimers(); });
describe('Expo Router mobile navigation', () => {
  it('opens a task and returns to the task list with its state retained', async () => {
    await renderRouter(routes, { initialUrl: '/' });
    await fireEvent.press(screen.getByRole('button', { name: 'Open shopping task' }));
    expect(screen.getByRole('button', { name: 'Back to tasks' })).toBeVisible();
    await fireEvent.press(screen.getByRole('button', { name: 'Decline proposal' }));
    await fireEvent.press(screen.getByRole('button', { name: 'Return to tasks' }));
    expect(screen.getByRole('button', { name: 'Open shopping task' })).toBeVisible();
    expect(screen.getByText('Declined')).toBeVisible();
  });
  it('loads a direct task link with a safe way back', async () => {
    await renderRouter(routes, { initialUrl: '/tasks/delhi' });
    expect(screen.getByText('Plan my flight to Delhi')).toBeVisible();
    await fireEvent.press(screen.getByRole('button', { name: 'Back to tasks' }));
    expect(screen.getByRole('button', { name: 'Open shopping task' })).toBeVisible();
  });
});
