import React, {useState} from 'react';
import {View, Text, ScrollView, StyleSheet, Alert} from 'react-native';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {ProfileHeader} from '../components/settings/ProfileHeader';
import {SettingsRow} from '../components/settings/SettingsRow';
import {GlassButton} from '../components/settings/GlassButton';
import {QrModal} from '../components/settings/QrModal';
import {UpdateRow} from '../components/settings/UpdateRow';
import {colors} from '../theme';

const soon = () => Alert.alert('Скоро', 'Эта часть появится позже.');

export function SettingsScreen() {
  const insets = useSafeAreaInsets();
  const [qrVisible, setQrVisible] = useState(false);
  const btnTop = insets.top + 10;

  return (
    <View style={s.root}>
      <ScrollView
        contentContainerStyle={{
          paddingTop: insets.top + 14,
          paddingHorizontal: 16,
          paddingBottom: 40,
        }}
        showsVerticalScrollIndicator={false}>
        <ProfileHeader name="Профиль" subtitle="P2P-идентичность появится с ядром" />

        <SettingsRow onPress={soon}>
          <View style={[s.icon, {backgroundColor: colors.accent}]}>
            <Ionicons name="camera" size={15} color="#fff" />
          </View>
          <Text style={s.blue}>Изменить фото</Text>
        </SettingsRow>

        <View style={s.gap} />
        <UpdateRow />

        <View style={s.gap} />
        <SettingsRow onPress={soon}>
          <View style={[s.icon, {backgroundColor: '#8b5cf6'}]}>
            <Ionicons name="lock-closed" size={15} color="#fff" />
          </View>
          <Text style={s.label}>Конфиденциальность</Text>
          <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
        </SettingsRow>
      </ScrollView>

      {/* QR (top-left) */}
      <View style={[s.overlay, {top: btnTop, left: 16}]}>
        <GlassButton onPress={() => setQrVisible(true)} style={s.qrBtn}>
          <Ionicons name="qr-code" size={24} color="#fff" />
        </GlassButton>
      </View>

      {/* Edit (top-right) */}
      <View style={[s.overlay, {top: btnTop, right: 16}]}>
        <GlassButton onPress={soon} style={s.pillBtn}>
          <Text style={s.editLabel}>Изм.</Text>
        </GlassButton>
      </View>

      <QrModal visible={qrVisible} onClose={() => setQrVisible(false)} />
    </View>
  );
}

const s = StyleSheet.create({
  root: {flex: 1, backgroundColor: colors.bg},
  icon: {width: 30, height: 30, borderRadius: 8, alignItems: 'center', justifyContent: 'center'},
  label: {flex: 1, color: colors.text, fontSize: 17},
  blue: {flex: 1, color: colors.accent, fontSize: 17},
  gap: {height: 10},
  overlay: {position: 'absolute', zIndex: 10},
  qrBtn: {width: 44, height: 44, borderRadius: 22},
  pillBtn: {height: 44, paddingHorizontal: 14, borderRadius: 22},
  editLabel: {color: colors.text, fontSize: 17, fontWeight: '500'},
});
