import { useState } from 'react';
import { Text } from 'react-native';
import { useTasks } from '../state/tasks';
import { Button, Card, Copy, Screen, Title, styles } from './ui';

export function ControlsScreen() {
  const { reset } = useTasks(); const [confirm, setConfirm] = useState(false), [message, setMessage] = useState('');
  return <Screen><Title eyebrow="ALWAYS YOUR CALL">Your boundaries.</Title>
    <Card><Text style={styles.heading}>Approval before action</Text><Copy>Every demo purchase needs an exact review and a separate confirmation. A budget does not grant permission by itself.</Copy></Card>
    <Card><Text style={styles.heading}>Examples and the connected lab</Text><Copy>The shopping and travel examples are fictional. The separate connected order lab can use a local test merchant and Stripe sandbox after pairing. Siri, Google Assistant, Swiggy and Zomato are not connected.</Copy></Card>
    <Card><Text style={styles.heading}>Your data in this preview</Text><Copy>Example decisions live in app memory. The connected lab requests microphone permission only when you press record, requires consent before hosted processing and keeps submitted text and transaction records on the computer. Disconnecting does not delete that history. No location, contacts or identity documents are requested.</Copy></Card>
    <Card><Text style={styles.heading}>Start fresh</Text><Copy>Reset both example tasks and their local activity.</Copy>{confirm ? <><Text style={styles.status}>Discard all demo decisions?</Text><Button title="Confirm reset" onPress={() => { reset(); setConfirm(false); setMessage('Demo tasks reset.'); }} /><Button secondary title="Keep my demo progress" onPress={() => setConfirm(false)} /></> : <Button secondary title="Reset demo tasks" onPress={() => { setMessage(''); setConfirm(true); }} />}{!!message && <Text accessibilityLiveRegion="polite" style={styles.status}>{message}</Text>}</Card>
  </Screen>;
}
