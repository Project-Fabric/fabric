import { Text, View } from 'react-native';
import { useTasks } from '../state/tasks';
import { Card, Copy, Screen, Title, styles } from './ui';
import { stageLabels } from './home-screen';

export function ActivityScreen() {
  const { tasks } = useTasks();
  return <Screen><Title eyebrow="EVERY STEP, IN VIEW">Your task activity.</Title><Copy>This is a local demo timeline, not a record of real transactions.</Copy>
    {Object.values(tasks).map(task => <Card key={task.example.id}><Text accessibilityRole="header" style={styles.heading}>{task.example.title}</Text><Text style={styles.status}>{stageLabels[task.stage]}</Text>
      {task.events.map((event, index) => <View key={index} style={{ gap: 5 }}><Text style={styles.small}>STEP {String(index + 1).padStart(2, '0')}</Text><Copy>{event}</Copy></View>)}
    </Card>)}
  </Screen>;
}
