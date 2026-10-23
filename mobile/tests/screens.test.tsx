import { AppState, BackHandler, Platform } from 'react-native';
import { act, fireEvent, render, screen, userEvent } from '@testing-library/react-native';
import { TasksProvider } from '../src/state/tasks';
import { TaskScreen } from '../src/components/task-screen';
import { HomeScreen } from '../src/components/home-screen';
import { ControlsScreen } from '../src/components/controls-screen';
import { ActivityScreen } from '../src/components/activity-screen';

const mount = async (id = 'headphones') => render(<TasksProvider><TaskScreen id={id} goBack={jest.fn()} /></TasksProvider>);
const press = async (name: string) => userEvent.setup().press(screen.getByRole('button', { name }));
const approve = async () => { await press('Review proposal'); await press('Approve demo proposal'); await press('Confirm demo approval'); };

describe('mobile screens', () => {
  it('requires two decisions, supports cancel, and completes only a demo purchase', async () => {
    await mount(); await press('Review proposal'); await press('Approve demo proposal');
    expect(screen.getByText('Confirm simulated approval')).toBeVisible();
    await press('Cancel approval');
    expect(screen.queryByRole('button', { name: 'Simulate success' })).toBeNull();
    await press('Approve demo proposal'); await press('Confirm demo approval'); await press('Simulate success');
    expect(screen.getByText('No real order or ticket was issued.')).toBeVisible();
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });
  it('disables review for an unsuitable flight and an over-budget offer', async () => {
    await mount('delhi');
    await userEvent.setup().press(screen.getByRole('radio', { name: /Early basic fare/ }));
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeDisabled();
    await userEvent.setup().press(screen.getByRole('radio', { name: /Morning nonstop/ }));
    await fireEvent.changeText(screen.getByLabelText('Example budget (INR)'), '10000');
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeDisabled();
    await press('Apply demo budget');
    expect(screen.getByRole('alert')).toHaveTextContent('The all-in total exceeds your task budget.');
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeDisabled();
  });
  it('reports invalid budgets without changing the current limit', async () => {
    await mount();
    await fireEvent.changeText(screen.getByLabelText('Example budget (INR)'), '-1');
    await press('Apply demo budget');
    expect(screen.getByRole('alert')).toHaveTextContent('Use a positive INR amount, with at most two decimal places.');
    expect(screen.getByText(/Current limit: INR 15,000.00/)).toBeVisible();
  });
  it('requires another approval after a changed price', async () => {
    await mount(); await approve(); await press('Simulate price change');
    expect(screen.queryByRole('button', { name: 'Simulate success' })).toBeNull();
    expect(screen.getByText('INR 13,499.00')).toBeVisible();
    await approve(); await press('Simulate success');
    expect(screen.getByText('No real order or ticket was issued.')).toBeVisible();
  });
  it('offers a status check rather than a second purchase after timeout', async () => {
    await mount(); await approve(); await press('Simulate timeout');
    expect(screen.getByText('Check before trying again.')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Simulate success' })).toBeNull();
    await press('Simulate status lookup');
    expect(screen.getByText('No real order or ticket was issued.')).toBeVisible();
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });
  (Platform.OS === 'android' ? it : it.skip)('uses Android Back to return from review to options', async () => {
    const listener = jest.spyOn(BackHandler, 'addEventListener');
    await mount(); await press('Review proposal');
    const callback = listener.mock.calls.at(-1)![1];
    await act(async () => {
      expect(callback({ type: 'hardwareBackPress', timeStamp: Date.now() })).toBe(true);
    });
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeVisible();
  });
  it('dismisses unfinished approval when the app goes into the background', async () => {
    const listener = jest.spyOn(AppState, 'addEventListener');
    await mount(); await press('Review proposal'); await press('Approve demo proposal');
    await act(async () => { listener.mock.calls[0][1]('background'); });
    expect(screen.queryByText('Confirm simulated approval')).toBeNull();
    expect(screen.getByRole('button', { name: 'Approve demo proposal' })).toBeVisible();
  });
  it('does not turn an unknown deep-linked ID into an action', async () => {
    await mount('__proto__');
    expect(screen.getByText('That example does not exist.')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Review proposal' })).toBeNull();
  });
  it('opens the chosen task from the mobile home screen', async () => {
    const open = jest.fn(); await render(<TasksProvider><HomeScreen openTask={open} /></TasksProvider>);
    await press('Open travel task'); expect(open).toHaveBeenCalledWith('delhi');
  });
  it('requires confirmation to reset and reflects decisions in Activity', async () => {
    const view = await render(<TasksProvider><TaskScreen id="headphones" goBack={jest.fn()} /></TasksProvider>);
    await press('Decline proposal');
    await view.rerender(<TasksProvider><ActivityScreen /></TasksProvider>);
    expect(screen.getByText('Declined')).toBeVisible();
    await view.rerender(<TasksProvider><ControlsScreen /></TasksProvider>);
    await press('Reset demo tasks'); await press('Keep my demo progress');
    await view.rerender(<TasksProvider><ActivityScreen /></TasksProvider>);
    expect(screen.getByText('Declined')).toBeVisible();
    await view.rerender(<TasksProvider><ControlsScreen /></TasksProvider>);
    await press('Reset demo tasks'); await press('Confirm reset');
    await view.rerender(<TasksProvider><ActivityScreen /></TasksProvider>);
    expect(screen.queryByText('Declined')).toBeNull();
  });
});
