import { router } from 'expo-router';
import { Button, Copy, Screen, Title } from '../components/ui';
export default function NotFound() { return <Screen><Title eyebrow="PAGE NOT FOUND">Let’s get back to your tasks.</Title><Copy>No action was taken.</Copy><Button title="Open tasks" onPress={() => router.replace('/')} /></Screen>; }
