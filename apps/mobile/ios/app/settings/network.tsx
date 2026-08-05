import { useEffect, useState } from 'react'
import { View, Text, ScrollView, Pressable, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import {
  p2pIsReady,
  p2pLocalPeerId,
  p2pLastStartupErrorDescription,
  p2pListenAddresses,
} from '../../modules/spiritchat-crypto-core'
import { useNetworkStore } from '../../store/network'

/**
 * What the network layer is actually doing — the screen that makes a
 * failed message diagnosable instead of a guess.
 *
 * The distinction this exists to draw: "the other person is offline",
 * "this node never started", and "both devices are online but have no path
 * to each other" all look identical from a chat that isn't delivering, and
 * they need completely different fixes. The reachability line below is the
 * one that answers the third case, which is the one that actually bites
 * two phones on mobile networks.
 */

/** How this device can be reached from outside its own network, if at all. */
type Reachability =
  | { kind: 'ipv6'; addresses: string[] }
  | { kind: 'relayed'; addresses: string[] }
  | { kind: 'local-only'; addresses: string[] }
  | { kind: 'none' }

function classifyReachability(addresses: string[]): Reachability {
  // A /p2p-circuit address is a relay reservation: reachable, at the cost
  // of going through someone else.
  const relayed = addresses.filter(a => a.includes('/p2p-circuit'))
  if (relayed.length > 0) return { kind: 'relayed', addresses: relayed }

  // A global IPv6 address means no NAT at all — the one path on which two
  // phones on mobile networks connect directly. Link-local (fe80::) and
  // loopback don't count; they never leave this link.
  const ipv6 = addresses.filter(
    a => a.startsWith('/ip6/') && !a.startsWith('/ip6/::1') && !a.startsWith('/ip6/fe80'),
  )
  if (ipv6.length > 0) return { kind: 'ipv6', addresses: ipv6 }

  const routable = addresses.filter(a => !a.startsWith('/ip4/127.') && !a.startsWith('/ip6/::1'))
  if (routable.length > 0) return { kind: 'local-only', addresses: routable }
  return { kind: 'none' }
}

const REACHABILITY_COPY: Record<Reachability['kind'], { label: string; detail: string; tone: 'good' | 'warn' | 'bad' }> = {
  ipv6: {
    label: 'Прямая связь (IPv6)',
    detail: 'У устройства есть глобальный IPv6-адрес. Далёкий собеседник может соединиться напрямую, без посредников.',
    tone: 'good',
  },
  relayed: {
    label: 'Через ретранслятор',
    detail: 'Прямого адреса нет, но зарезервирован канал через другой узел — далёкий собеседник дозвонится через него.',
    tone: 'good',
  },
  'local-only': {
    label: 'Только локальная сеть',
    detail: 'Есть только адреса за NAT. Собеседник в этой же сети дозвонится, далёкий — нет.',
    tone: 'warn',
  },
  none: {
    label: 'Недостижимо',
    detail: 'Нет ни одного адреса, по которому до этого устройства можно дозвониться.',
    tone: 'bad',
  },
}

const TONE_COLOR = { good: '#22c55e', warn: '#f59e0b', bad: '#ef4444' } as const

export default function NetworkScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()

  const connectedPeers = useNetworkStore(s => s.connectedPeers)
  const locallyDiscovered = useNetworkStore(s => s.locallyDiscovered)
  const everConnected = useNetworkStore(s => s.everConnected)
  const chainHeight = useNetworkStore(s => s.chainHeight)
  const lastDialFailure = useNetworkStore(s => s.lastDialFailure)

  // Read through the bridge rather than mirrored into the store: these are
  // synchronous native reads with no event to drive them, and a stale
  // value on a diagnostics screen is worse than no value.
  const [snapshot, setSnapshot] = useState(() => readNativeSnapshot())
  useEffect(() => {
    const timer = setInterval(() => setSnapshot(readNativeSnapshot()), 2000)
    return () => clearInterval(timer)
  }, [])

  const reachability = classifyReachability(snapshot.listenAddresses)
  const copy = REACHABILITY_COPY[reachability.kind]

  return (
    <View style={[s.root, { paddingTop: insets.top + 10 }]}>
      <View style={s.header}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Text style={s.title}>Сеть</Text>
      </View>

      <ScrollView contentContainerStyle={{ paddingBottom: insets.bottom + 40, paddingHorizontal: 20, gap: 24 }}>

        <Section title="Достижимость">
          <View style={s.row}>
            <View style={[s.dot, { backgroundColor: TONE_COLOR[copy.tone] }]} />
            <Text style={s.rowValue}>{copy.label}</Text>
          </View>
          <Text style={s.hint}>{copy.detail}</Text>
        </Section>

        <Section title="Узел">
          <Field label="Состояние" value={snapshot.ready ? 'запущен' : 'не запущен'} />
          {snapshot.startupError ? (
            <Field label="Ошибка запуска" value={snapshot.startupError} mono />
          ) : null}
          <Field label="Peer ID" value={snapshot.peerId ?? '—'} mono />
          <Field
            label="Высота цепочки"
            value={chainHeight === null ? 'нет данных' : String(chainHeight)}
          />
          <Text style={s.hint}>
            Поиск по @username отвечает из локальной копии цепочки. Пока она не
            синхронизирована с кем-то, чужое имя не найдётся.
          </Text>
        </Section>

        <Section title={`Соединения (${connectedPeers.length})`}>
          {connectedPeers.length === 0 ? (
            <Text style={s.hint}>
              {everConnected
                ? 'Сейчас никого. Соединения были с момента запуска.'
                : 'С момента запуска не соединились ни с кем. Сообщения уйдут в офлайн-доставку и будут ждать.'}
            </Text>
          ) : (
            connectedPeers.map(peerId => <Text key={peerId} style={s.mono}>{peerId}</Text>)
          )}
        </Section>

        <Section title={`Слушает (${snapshot.listenAddresses.length})`}>
          {snapshot.listenAddresses.length === 0 ? (
            <Text style={s.hint}>Нет адресов.</Text>
          ) : (
            snapshot.listenAddresses.map(addr => <Text key={addr} style={s.mono}>{addr}</Text>)
          )}
        </Section>

        {locallyDiscovered.length > 0 ? (
          <Section title={`Найдены в локальной сети (${locallyDiscovered.length})`}>
            {locallyDiscovered.map(peerId => <Text key={peerId} style={s.mono}>{peerId}</Text>)}
          </Section>
        ) : null}

        {lastDialFailure ? (
          <Section title="Последняя неудачная попытка">
            <Field label="Пир" value={lastDialFailure.peerId ?? 'неизвестен'} mono />
            <Field label="Причина" value={lastDialFailure.reason} mono />
          </Section>
        ) : null}

      </ScrollView>
    </View>
  )
}

function readNativeSnapshot() {
  // Every one of these throws when there is no live session (app still
  // starting, or signed out), which is itself a state worth showing rather
  // than crashing the screen over.
  let ready = false
  try { ready = p2pIsReady() } catch {}

  let peerId: string | null = null
  try { peerId = p2pLocalPeerId() } catch {}

  let startupError: string | null = null
  try { startupError = p2pLastStartupErrorDescription() } catch {}

  let listenAddresses: string[] = []
  try { listenAddresses = p2pListenAddresses() } catch {}

  return { ready, peerId, startupError, listenAddresses }
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <View style={{ gap: 8 }}>
      <Text style={s.sectionTitle}>{title}</Text>
      <View style={s.card}>{children}</View>
    </View>
  )
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <View style={{ gap: 2 }}>
      <Text style={s.fieldLabel}>{label}</Text>
      <Text style={mono ? s.mono : s.rowValue} selectable>{value}</Text>
    </View>
  )
}

const s = StyleSheet.create({
  root:    { flex: 1, backgroundColor: '#000' },
  header:  { flexDirection: 'row', alignItems: 'center', gap: 14, paddingHorizontal: 16, paddingBottom: 16 },
  backBtn: { width: 40, height: 40, borderRadius: 20, alignItems: 'center', justifyContent: 'center' },
  title:   { color: '#fff', fontSize: 24, fontWeight: '700' },

  sectionTitle: { color: 'rgba(255,255,255,0.5)', fontSize: 13, fontWeight: '600', textTransform: 'uppercase', letterSpacing: 0.5 },
  card:    { backgroundColor: '#111114', borderRadius: 14, padding: 14, gap: 12 },

  row:       { flexDirection: 'row', alignItems: 'center', gap: 10 },
  dot:       { width: 10, height: 10, borderRadius: 5 },
  rowValue:  { color: '#fff', fontSize: 16 },
  fieldLabel:{ color: 'rgba(255,255,255,0.45)', fontSize: 12 },
  mono:      { color: '#fff', fontSize: 12, fontFamily: 'Menlo' },
  hint:      { color: 'rgba(255,255,255,0.45)', fontSize: 13, lineHeight: 18 },
})
