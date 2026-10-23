import { router, useLocalSearchParams } from 'expo-router';
import { TaskScreen } from '../../components/task-screen';
export default function Task() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <TaskScreen id={typeof id === 'string' ? id : ''} goBack={() => router.canGoBack() ? router.back() : router.replace('/')} />;
}
