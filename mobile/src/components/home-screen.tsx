import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useTasks } from '../state/tasks';
import { taskMoney, type TaskStage } from '../domain/task-fixtures';
import { Button, Card, Copy, DemoNotice, Screen, Title, colors, styles } from './ui';

export const stageLabels: Record<TaskStage, string> = { research: 'Compare options', review: 'Needs your approval', approved: 'Approved in demo', unknown: 'Needs a status check', completed: 'Demo completed', rejected: 'Declined' };
export function HomeScreen({ openTask, openLab }: { openTask: (id: string) => void; openLab?: () => void }) {
  const { tasks } = useTasks();
  return <Screen><Title eyebrow="A LITTLE LESS TO DO">Your day,{"\n"}with more room.</Title><Copy>Give a task a direction. Keep the decisions yours.</Copy><DemoNotice />
    {openLab && <Card><Text style={styles.heading}>Test a voice purchase on your phone.</Text><Copy>Pair with your computer to use the test merchant, server budget controls and optional Stripe sandbox. No real funds or food orders.</Copy><Button title="Open connected order lab" onPress={openLab} /></Card>}
    <View style={home.section}><Text accessibilityRole="header" style={styles.heading}>Try a task</Text><Text style={styles.small}>2 examples</Text></View>
    {Object.values(tasks).map(task => <Pressable key={task.example.id} accessibilityRole="button" accessibilityLabel={`Open ${task.example.kind} task`} onPress={() => openTask(task.example.id)} style={({ pressed }) => pressed && styles.pressed}>
      <Card><View style={home.section}><Text style={styles.eyebrow}>{task.example.kind === 'shopping' ? '01 / SHOPPING' : '02 / TRAVEL'}</Text><Text style={home.arrow} accessible={false}>↗</Text></View><Text style={styles.heading}>{task.example.title}</Text><Copy>{task.example.kind === 'shopping' ? 'The right headphones, with the tradeoffs in view.' : 'A flight that fits your plans, including the checked bag.'}</Copy><View style={styles.divider} /><Text style={styles.status}>{stageLabels[task.stage]}</Text><Text style={styles.small}>Task budget · {taskMoney(task.budget)}</Text></Card>
    </Pressable>)}
    <Card><Text style={styles.heading}>Built around your permission.</Text><Copy>Review the exact option before a simulated purchase. If the price changes, approval starts again.</Copy></Card>
  </Screen>;
}
const home = StyleSheet.create({ section: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 14 }, arrow: { color: colors.accent, fontSize: 26 } });
