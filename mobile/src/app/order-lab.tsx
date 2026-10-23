import { useCallback, useState } from 'react';
import { router, useFocusEffect, useLocalSearchParams } from 'expo-router';
import { OrderLabScreen } from '../components/order-lab-screen';
import { assistantRequest } from '../domain/assistant-request';

export default function OrderLab() {
  const [focused, setFocused] = useState(true);
  const params = useLocalSearchParams<{ assistantRequest?: string | string[]; assistantInvocation?: string }>();
  useFocusEffect(useCallback(() => { setFocused(true); return () => setFocused(false); }, []));
  return <OrderLabScreen focused={focused} incomingRequest={assistantRequest(params.assistantRequest)} incomingId={params.assistantInvocation}
    goBack={() => { if (router.canGoBack()) router.back(); else router.replace('/'); }} />;
}
