import { Redirect, router } from 'expo-router';
import { HomeScreen } from '../../components/home-screen';
export default function Home() {
  if (process.env.EXPO_PUBLIC_COSIGNA_NATIVE_APP === '1') return <Redirect href="/order-lab" />;
  return <HomeScreen openTask={id => router.push({ pathname: '/tasks/[id]', params: { id } })} openLab={() => router.push('/order-lab')} />;
}
