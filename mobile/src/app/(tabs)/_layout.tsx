import { Tabs } from 'expo-router';
import { Text } from 'react-native';
import { colors } from '../../components/ui';

export default function TabLayout() {
  return <Tabs screenOptions={{ headerShown: false, tabBarActiveTintColor: colors.accent, tabBarInactiveTintColor: colors.muted,
    tabBarStyle: { backgroundColor: colors.bg, borderTopColor: colors.line }, tabBarLabelStyle: { fontSize: 12 } }}>
    <Tabs.Screen name="index" options={{ title: 'Tasks', tabBarIcon: ({ color }) => <Text style={{ color, fontSize: 22 }} accessible={false}>◎</Text> }} />
    <Tabs.Screen name="activity" options={{ title: 'Activity', tabBarIcon: ({ color }) => <Text style={{ color, fontSize: 22 }} accessible={false}>≡</Text> }} />
    <Tabs.Screen name="controls" options={{ title: 'Controls', tabBarIcon: ({ color }) => <Text style={{ color, fontSize: 22 }} accessible={false}>◇</Text> }} />
  </Tabs>;
}
