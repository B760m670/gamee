import React from 'react';
import {Modal, View, Text, Pressable, StyleSheet} from 'react-native';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {colors} from '../../theme';

interface Props {
  visible: boolean;
  onClose: () => void;
}

// Identity QR placeholder. The real QR (encoding the P2P public key) is wired
// once the native P2P core lands.
export function QrModal({visible, onClose}: Props) {
  return (
    <Modal visible={visible} transparent animationType="fade" onRequestClose={onClose}>
      <View style={s.backdrop}>
        <View style={s.card}>
          <View style={s.qr}>
            <Ionicons name="qr-code-outline" size={120} color={colors.faint} />
          </View>
          <Text style={s.title}>Ваш QR-код</Text>
          <Text style={s.note}>
            Появится, когда добавим P2P-идентичность — по нему смогут добавить
            ваш профиль.
          </Text>
          <Pressable style={s.btn} onPress={onClose}>
            <Text style={s.btnText}>Закрыть</Text>
          </Pressable>
        </View>
      </View>
    </Modal>
  );
}

const s = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.7)',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 32,
  },
  card: {
    width: '100%',
    backgroundColor: '#1c1c1e',
    borderRadius: 24,
    padding: 24,
    alignItems: 'center',
    gap: 8,
  },
  qr: {
    width: 180,
    height: 180,
    borderRadius: 16,
    backgroundColor: 'rgba(255,255,255,0.04)',
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 8,
  },
  title: {color: colors.text, fontSize: 18, fontWeight: '700'},
  note: {color: colors.textMuted, fontSize: 14, textAlign: 'center', lineHeight: 19},
  btn: {
    marginTop: 12,
    backgroundColor: colors.accent,
    borderRadius: 12,
    paddingHorizontal: 24,
    paddingVertical: 10,
  },
  btnText: {color: '#ffffff', fontSize: 15, fontWeight: '600'},
});
