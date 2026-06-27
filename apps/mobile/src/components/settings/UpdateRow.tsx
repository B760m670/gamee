import React from 'react';
import {View, Text, ActivityIndicator, StyleSheet} from 'react-native';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {SettingsRow} from './SettingsRow';
import {useUpdate} from '../../update/useUpdate';
import {APP_VERSION} from '../../update/appVersion';
import {colors} from '../../theme';

// Settings row that shows the current version and lets the user check for a
// newer release on demand.
export function UpdateRow() {
  const {checking, available, latest, error, check} = useUpdate();

  const status = checking
    ? 'Проверка…'
    : error
    ? 'Не удалось проверить'
    : available
    ? `Доступно ${latest?.tag}`
    : `Версия ${APP_VERSION} — актуально`;

  return (
    <SettingsRow onPress={check}>
      <View style={[s.icon, {backgroundColor: '#34c759'}]}>
        <Ionicons name="cloud-download" size={15} color="#fff" />
      </View>
      <View style={s.text}>
        <Text style={s.label}>Обновления</Text>
        <Text style={[s.sub, available && {color: colors.accent}]}>{status}</Text>
      </View>
      {checking ? (
        <ActivityIndicator color={colors.textMuted} />
      ) : (
        <Ionicons name="refresh" size={18} color="rgba(255,255,255,0.4)" />
      )}
    </SettingsRow>
  );
}

const s = StyleSheet.create({
  icon: {width: 30, height: 30, borderRadius: 8, alignItems: 'center', justifyContent: 'center'},
  text: {flex: 1},
  label: {color: colors.text, fontSize: 17},
  sub: {color: colors.textMuted, fontSize: 13, marginTop: 1},
});
