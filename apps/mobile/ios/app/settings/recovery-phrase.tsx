import { useEffect, useState } from 'react'
import {
  View, Text, Pressable, ScrollView, StyleSheet, Share,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { recoveryPhraseWords } from '../../modules/spiritchat-crypto-core'

const BTN_H = 44

export default function RecoveryPhraseScreen() {
  const router  = useRouter()
  const insets  = useSafeAreaInsets()
  const [revealed, setRevealed] = useState(false)
  const [words, setWords]       = useState<string[] | null>(null)

  useEffect(() => {
    const stored = recoveryPhraseWords()
    setWords(stored ? stored.split(' ') : [])
  }, [])

  async function handleShare() {
    if (!words || words.length === 0) return
    try { await Share.share({ message: words.join(' ') }) } catch {}
  }

  return (
    <View style={s.root}>
      <View style={[s.btnOverlay, { top: insets.top + 10, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 70, paddingBottom: insets.bottom + 40, paddingHorizontal: 24 }}
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Фраза восстановления</Text>
        <Text style={s.subtitle}>
          Это единственный способ восстановить аккаунт на новом устройстве. Никому её не показывай — тот, кто её узнает, получит полный доступ к твоему аккаунту.
        </Text>

        {!revealed ? (
          <Pressable style={s.revealBtn} onPress={() => setRevealed(true)}>
            <Ionicons name="eye-outline" size={18} color="#2f7bff" />
            <Text style={s.revealText}>Показать фразу</Text>
          </Pressable>
        ) : words === null ? null : words.length === 0 ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>
              Фраза недоступна на этом устройстве. Она сохраняется в тот же момент, что и сам аккаунт — если ты видишь это сообщение, что-то пошло не так при создании или восстановлении.
            </Text>
          </View>
        ) : (
          <>
            <View style={s.grid}>
              {words.map((word, i) => (
                <View key={i} style={s.wordCell}>
                  <Text style={s.wordIndex}>{i + 1}</Text>
                  <Text style={s.wordText}>{word}</Text>
                </View>
              ))}
            </View>

            <Pressable style={({ pressed }) => [s.copyBtn, pressed && s.copyBtnPressed]} onPress={handleShare}>
              <Ionicons name="share-outline" size={16} color="#2f7bff" />
              <Text style={s.copyText}>Поделиться</Text>
            </Pressable>
          </>
        )}
      </ScrollView>
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  title:    { color: '#fff', fontSize: 26, fontWeight: '700', marginBottom: 10 },
  subtitle: { color: '#71717a', fontSize: 14, lineHeight: 20, marginBottom: 24 },

  revealBtn: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8,
    backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a',
    borderRadius: 14, paddingVertical: 16,
  },
  revealText: { color: '#2f7bff', fontSize: 16, fontWeight: '600' },

  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: 10, marginBottom: 20 },
  wordCell: {
    width: '47%', flexDirection: 'row', alignItems: 'center', gap: 8,
    backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a',
    borderRadius: 10, paddingHorizontal: 12, paddingVertical: 12,
  },
  wordIndex: { color: '#52525b', fontSize: 13, width: 18 },
  wordText:  { color: '#ffffff', fontSize: 15, fontWeight: '600' },

  copyBtn: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8,
    paddingVertical: 12,
  },
  copyBtnPressed: { opacity: 0.7 },
  copyText: { color: '#2f7bff', fontSize: 15, fontWeight: '500' },

  errorBox:  { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12 },
  errorText: { color: '#f87171', fontSize: 14, lineHeight: 20 },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
})
