import { useEffect, useState } from 'react';
import { AppState, BackHandler, Keyboard, Modal, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { parseTaskBudget, selectedOption, taskBlocker, taskMoney, taskTotal, type TaskState } from '../domain/task-fixtures';
import { useTasks } from '../state/tasks';
import { Button, Card, Copy, DemoNotice, Screen, Title, colors, styles, useReducedMotion } from './ui';
import { stageLabels } from './home-screen';

export function TaskScreen({ id, goBack }: { id: string; goBack: () => void }) {
  const { tasks } = useTasks();
  const task = Object.hasOwn(tasks, id) ? tasks[id] : null;
  if (!task) return <Screen><Title eyebrow="TASK UNAVAILABLE">That example does not exist.</Title><Copy>No action was taken.</Copy><Button title="Back to tasks" onPress={goBack} /></Screen>;
  return <TaskDetail key={id} task={task} goBack={goBack} />;
}

function TaskDetail({ task, goBack }: { task: TaskState; goBack: () => void }) {
  const { update } = useTasks();
  const [budget, setBudget] = useState(String(task.budget / 100)), [error, setError] = useState('');
  const [confirmation, setConfirmation] = useState<number | null>(null);
  const reducedMotion = useReducedMotion();
  const id = task.example.id;
  const send = (action: Parameters<typeof update>[1]) => update(id, action);
  const option = selectedOption(task), total = taskTotal(task), blocker = taskBlocker(task);
  const budgetDirty = budget !== String(task.budget / 100);
  useEffect(() => {
    const listener = AppState.addEventListener('change', state => { if (state !== 'active') setConfirmation(null); });
    return () => listener.remove();
  }, []);
  useEffect(() => {
    const listener = BackHandler.addEventListener('hardwareBackPress', () => {
      if (task.stage === 'review') { update(id, { type: 'edit' }); return true; }
      return false;
    });
    return () => listener.remove();
  }, [id, task.stage, update]);
  const index = task.stage === 'research' ? 0 : ['review', 'rejected'].includes(task.stage) ? 1 : task.stage === 'completed' ? 3 : 2;
  const applyBudget = () => {
    const amount = parseTaskBudget(budget);
    if (amount === null) { setError('Use a positive INR amount, with at most two decimal places.'); return; }
    send({ type: 'budget', amount }); setBudget(String(amount / 100)); setError(''); Keyboard.dismiss();
  };
  return <><Screen resetKey={task.stage}><Button title="Back to tasks" onPress={goBack} secondary />
    <Title eyebrow={task.example.kind === 'travel' ? 'YOUR TRAVEL TASK' : 'YOUR SHOPPING TASK'}>{task.example.title}</Title>
    <Text accessibilityLiveRegion="polite" style={styles.status}>{stageLabels[task.stage]}</Text><DemoNotice />
    <Card><Text style={styles.eyebrow}>THE REQUEST</Text><Copy>{task.example.request}</Copy></Card>
    <View style={detail.progress} accessibilityLabel={`Step ${index + 1} of 4: ${stageLabels[task.stage]}`} accessible>
      {['Research', 'Approve', 'Execute', 'Result'].map((label, i) => <View key={label} style={detail.step}><View style={[detail.stepLine, i <= index && detail.stepActive]} /><Text style={styles.small}>{label}</Text></View>)}
    </View>
    {task.stage === 'research' && <>
      <Card><Text accessibilityRole="header" style={styles.heading}>Set your task budget.</Text><Text nativeID="budget-label" style={styles.small}>Example budget (INR)</Text>
        <TextInput accessibilityLabel="Example budget in INR" accessibilityLabelledBy="budget-label" keyboardType="decimal-pad" value={budget} onChangeText={value => { setBudget(value); setError(''); }} style={detail.input} selectionColor={colors.accent} maxLength={10} onSubmitEditing={applyBudget} />
        {!!error && <Text accessibilityRole="alert" style={styles.error}>{error}</Text>}
        <Button secondary title="Apply demo budget" onPress={applyBudget} /><Text style={styles.small}>Current limit: {taskMoney(task.budget)} · includes taxes and fees</Text>
      </Card>
      <Text accessibilityRole="header" style={styles.heading}>Compare the options.</Text><Copy>These offers are made up for the demo. No live availability has been checked.</Copy>
      {task.example.options.map(item => <Pressable key={item.id} accessibilityRole="radio" accessibilityLabel={`${item.title}, ${taskMoney(item.total + task.priceIncrease)}`} accessibilityState={{ checked: task.selected === item.id }} onPress={() => send({ type: 'select', id: item.id })} style={({ pressed }) => [detail.offer, task.selected === item.id && detail.offerSelected, pressed && styles.pressed]}>
        <View style={styles.row}><View style={[detail.radio, task.selected === item.id && detail.radioSelected]} /><Text style={[styles.heading, { flex: 1 }]}>{item.title}</Text></View><Text style={styles.small}>{item.provider} · fictional</Text><Text style={styles.price}>{taskMoney(item.total + task.priceIncrease)}</Text>
        {item.details.map(text => <Text key={text} style={styles.copy}>{text}</Text>)}
        {task.priceIncrease > 0 && <Text style={styles.small}>Updated quote includes an additional {taskMoney(task.priceIncrease)}.</Text>}
        <Text style={item.mismatch ? detail.warning : styles.status}>{item.mismatch || 'Matches the example requirements'}</Text>
      </Pressable>)}
      {blocker && <Text accessibilityRole="alert" style={detail.warning}>{blocker}</Text>}
      {budgetDirty && <Text style={styles.small}>Apply your budget edit before reviewing the proposal.</Text>}
      <Button title="Review proposal" disabled={!!blocker || budgetDirty} onPress={() => { Keyboard.dismiss(); send({ type: 'review' }); }} />
      <Button secondary title="Decline proposal" onPress={() => send({ type: 'reject' })} />
    </>}
    {task.stage === 'review' && <Card><Text style={styles.eyebrow}>ONE OPTION. ONE APPROVAL.</Text><Text accessibilityRole="header" style={styles.heading}>Review the exact proposal.</Text><Copy>{option.title} from {option.provider}</Copy><Text style={styles.price}>{taskMoney(total)}</Text><Copy>All-in demo total · quote {task.quoteVersion}</Copy>
      {option.details.map(text => <Copy key={text}>{text}</Copy>)}{task.priceIncrease > 0 && <Copy>Includes an additional {taskMoney(task.priceIncrease)} from the changed quote.</Copy>}
      <Text style={styles.heading}>Your conditions</Text>{task.example.constraints.map(text => <Copy key={text}>{text}</Copy>)}
      <Button title="Approve demo proposal" onPress={() => setConfirmation(task.quoteVersion)} />
      <Button secondary title="Back to options" onPress={() => send({ type: 'edit' })} /><Button secondary title="Decline proposal" onPress={() => send({ type: 'reject' })} />
    </Card>}
    {task.stage === 'approved' && <Card><Text style={styles.eyebrow}>SIMULATED APPROVAL RECORDED</Text><Text accessibilityRole="header" style={styles.heading}>What happens next?</Text><Copy>No request has gone to a provider. Pick an outcome to explore how the task should behave.</Copy><Text style={styles.small}>Reserved in demo: {taskMoney(total)}</Text>
      <Button title="Simulate success" onPress={() => send({ type: 'complete' })} />
      <Button secondary title="Simulate price change" hint="Adds INR 500 and requires another approval" onPress={() => send({ type: 'price-change' })} />
      <Button secondary title="Simulate timeout" onPress={() => send({ type: 'unknown' })} />
    </Card>}
    {task.stage === 'unknown' && <Card><Text style={styles.eyebrow}>OUTCOME UNKNOWN</Text><Text accessibilityRole="header" style={styles.heading}>Check before trying again.</Text><Copy>A provider timeout does not tell us whether a purchase succeeded. This demo keeps {taskMoney(total)} reserved until the original request is resolved.</Copy><Copy>The next step checks the existing order; it does not submit another purchase.</Copy><Button title="Simulate status lookup" onPress={() => send({ type: 'reconcile' })} /></Card>}
    {task.stage === 'completed' && <Card><Text style={styles.eyebrow}>DEMO COMPLETED</Text><Text accessibilityRole="header" style={styles.heading}>A result you can inspect.</Text><Copy>{task.events.at(-1)}</Copy><Text style={styles.price}>{taskMoney(total)}</Text><Text style={styles.small}>Example confirmation: DEMO-{id.toUpperCase()}-001</Text><Text style={styles.status}>No real order or ticket was issued.</Text><Button secondary title="Return to tasks" onPress={goBack} /></Card>}
    {task.stage === 'rejected' && <Card><Text accessibilityRole="header" style={styles.heading}>You chose not to proceed.</Text><Copy>No order submitted. No funds reserved. You can reset the examples from Controls.</Copy><Button secondary title="Return to tasks" onPress={goBack} /></Card>}
    <Card><Text accessibilityRole="header" style={styles.heading}>Task activity</Text>{task.events.map((event, i) => <View key={i} style={{ gap: 5 }}><Text style={styles.small}>STEP {i + 1}</Text><Copy>{event}</Copy></View>)}</Card>
  </Screen>
  <Modal visible={confirmation !== null} transparent animationType={reducedMotion ? 'none' : 'fade'} onRequestClose={() => setConfirmation(null)}>
    <View style={detail.scrim}><SafeAreaView style={detail.modalSafe}><ScrollView contentContainerStyle={detail.modalScroll}><View style={detail.sheet} accessibilityViewIsModal>
      <Text style={styles.eyebrow}>THIS PREVIEW ONLY</Text><Text accessibilityRole="header" style={styles.heading}>Confirm simulated approval</Text><Copy>{option.title} · {option.provider}</Copy><Text style={styles.price}>{taskMoney(total)}</Text><Copy>Quote {confirmation} · one purchase. This is not a payment or an authenticated spending grant.</Copy>
      <Button secondary title="Cancel approval" onPress={() => setConfirmation(null)} />
      <Button title="Confirm demo approval" onPress={() => { if (confirmation !== null) send({ type: 'approve', quoteVersion: confirmation }); setConfirmation(null); }} />
    </View></ScrollView></SafeAreaView></View>
  </Modal></>;
}
const detail = StyleSheet.create({
  progress: { flexDirection: 'row', gap: 8 }, step: { flex: 1, gap: 8 }, stepLine: { height: 3, backgroundColor: colors.line, borderRadius: 3 }, stepActive: { backgroundColor: colors.accent },
  input: { minHeight: 52, padding: 13, borderWidth: 1, borderColor: colors.muted, borderRadius: 12, backgroundColor: colors.bg, color: colors.ink, fontSize: 18 },
  offer: { borderWidth: 1, borderColor: colors.line, borderRadius: 20, padding: 21, backgroundColor: colors.panel, gap: 12 },
  offerSelected: { borderColor: colors.accent, borderWidth: 2, padding: 20 },
  radio: { width: 20, height: 20, borderRadius: 10, borderWidth: 2, borderColor: colors.muted }, radioSelected: { borderColor: colors.accent, backgroundColor: colors.accent },
  warning: { color: colors.amber, fontSize: 13, lineHeight: 21 },
  scrim: { flex: 1, backgroundColor: '#000b', justifyContent: 'center' }, modalSafe: { flex: 1 },
  modalScroll: { flexGrow: 1, justifyContent: 'center', padding: 20 },
  sheet: { padding: 24, gap: 20, borderRadius: 24, backgroundColor: colors.panel, borderColor: colors.line, borderWidth: 1, maxWidth: 600, width: '100%', alignSelf: 'center' },
});
