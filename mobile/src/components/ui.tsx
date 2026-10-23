import { useEffect, useRef, useState, type ComponentRef, type ReactNode } from 'react';
import { AccessibilityInfo, KeyboardAvoidingView, Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';

export const colors = { bg: '#111614', panel: '#1b221e', raised: '#242e28', ink: '#f2f1e8', muted: '#b1baaf',
  line: '#465248', accent: '#d1e5b4', onAccent: '#182015', amber: '#f4cd8f', error: '#ffb8b0' };
export function useReducedMotion() {
  const [reduced, setReduced] = useState(true);
  useEffect(() => {
    let mounted = true;
    void AccessibilityInfo.isReduceMotionEnabled().then(value => { if (mounted) setReduced(value); });
    const listener = AccessibilityInfo.addEventListener('reduceMotionChanged', setReduced);
    return () => { mounted = false; listener.remove(); };
  }, []);
  return reduced;
}
export function Screen({ children, resetKey }: { children: ReactNode; resetKey?: string }) {
  const scroll = useRef<ComponentRef<typeof ScrollView>>(null);
  useEffect(() => { scroll.current?.scrollTo({ y: 0, animated: false }); }, [resetKey]);
  return <SafeAreaView edges={['top', 'left', 'right']} style={styles.screen}>
    <KeyboardAvoidingView style={styles.screen} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
      <ScrollView ref={scroll} keyboardShouldPersistTaps="handled" contentContainerStyle={styles.content}>
        <View style={styles.brandRow}><View style={styles.brandMark} accessible={false}><View style={styles.brandInner} /></View><Text style={styles.brand}>cosigna</Text><Text style={styles.preview}>MOBILE PREVIEW</Text></View>
        {children}
      </ScrollView>
    </KeyboardAvoidingView>
  </SafeAreaView>;
}
export function Title({ eyebrow, children }: { eyebrow: string; children: ReactNode }) {
  return <View style={styles.titleBlock}><Text style={styles.eyebrow}>{eyebrow}</Text><Text accessibilityRole="header" style={styles.title}>{children}</Text></View>;
}
export function Copy({ children }: { children: ReactNode }) { return <Text style={styles.copy}>{children}</Text>; }
export function Card({ children }: { children: ReactNode }) { return <View style={styles.card}>{children}</View>; }
export function Button({ title, onPress, secondary = false, disabled = false, hint }: {
  title: string; onPress: () => void; secondary?: boolean; disabled?: boolean; hint?: string;
}) {
  return <Pressable accessibilityRole="button" accessibilityLabel={title} accessibilityHint={hint}
    accessibilityState={{ disabled }} disabled={disabled} onPress={onPress}
    style={({ pressed }) => [styles.button, secondary && styles.secondary, disabled && styles.disabled, pressed && !disabled && styles.pressed]}>
    <Text style={[styles.buttonText, secondary && styles.secondaryText]}>{title}</Text>
  </Pressable>;
}
export function DemoNotice() {
  return <View style={styles.notice}><Text style={styles.noticeTitle}>A working preview. Fictional transactions.</Text><Copy>No real searches, purchases or bookings. Your demo progress stays in memory until the app reloads.</Copy></View>;
}
export const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: colors.bg },
  content: { padding: 22, paddingBottom: 38, gap: 18, width: '100%', maxWidth: 700, alignSelf: 'center' },
  brandRow: { flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap', gap: 10, paddingVertical: 8 },
  brand: { color: colors.ink, fontSize: 25, letterSpacing: -1, fontWeight: '600' },
  brandMark: { width: 25, height: 25, borderRadius: 16, borderColor: colors.accent, borderWidth: 1.5, transform: [{ rotate: '-30deg' }], justifyContent: 'center', alignItems: 'center' },
  brandInner: { width: 12, height: 25, borderRadius: 12, borderWidth: 1.5, borderColor: colors.accent },
  preview: { color: colors.muted, fontSize: 9, letterSpacing: 1.5, marginLeft: 'auto' },
  titleBlock: { gap: 12, paddingTop: 16, paddingBottom: 6 },
  eyebrow: { color: colors.accent, fontSize: 10, letterSpacing: 2, fontWeight: '600' },
  title: { fontSize: 38, lineHeight: 44, letterSpacing: -1.4, color: colors.ink, fontWeight: '500' },
  heading: { fontSize: 22, lineHeight: 29, letterSpacing: -0.4, color: colors.ink, fontWeight: '500' },
  copy: { fontSize: 14, lineHeight: 22, color: colors.muted },
  card: { backgroundColor: colors.panel, borderRadius: 20, borderWidth: 1, borderColor: colors.line, padding: 21, gap: 14 },
  row: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: 10 },
  small: { color: colors.muted, fontSize: 12, lineHeight: 19 },
  status: { color: colors.accent, fontSize: 12, fontWeight: '500', lineHeight: 18 },
  button: { backgroundColor: colors.accent, minHeight: 50, borderRadius: 14, alignItems: 'center', justifyContent: 'center', paddingHorizontal: 16, paddingVertical: 14 },
  buttonText: { color: colors.onAccent, fontSize: 14, lineHeight: 21, fontWeight: '600', textAlign: 'center' },
  secondary: { backgroundColor: colors.raised, borderWidth: 1, borderColor: colors.line },
  secondaryText: { color: colors.ink },
  disabled: { opacity: 0.45 }, pressed: { opacity: 0.75, transform: [{ scale: 0.99 }] },
  notice: { padding: 17, borderRadius: 15, backgroundColor: colors.raised, gap: 6 },
  noticeTitle: { color: colors.ink, fontSize: 13, fontWeight: '500', lineHeight: 21 },
  divider: { height: 1, backgroundColor: colors.line, marginVertical: 3 },
  price: { fontSize: 30, color: colors.ink, letterSpacing: -1, fontWeight: '500' },
  error: { color: colors.error, fontSize: 13, lineHeight: 21 },
});
