import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { TasksProvider } from '../state/tasks';
import { colors, useReducedMotion } from '../components/ui';

export default function RootLayout() {
  const reducedMotion = useReducedMotion();
  return <TasksProvider><StatusBar style="light" /><Stack screenOptions={{ headerShown: false, contentStyle: { backgroundColor: colors.bg }, animation: reducedMotion ? 'none' : 'fade' }}>
    <Stack.Screen name="(tabs)" /><Stack.Screen name="tasks/[id]" />
  </Stack></TasksProvider>;
}
