import React from 'react';
import {View, Text, Pressable, StyleSheet} from 'react-native';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import {colors} from '../theme';

interface Props {
  bytes: number;
  total: number;
  paused: boolean;
  error: boolean;
  onCancel: () => void;
}

const mb = (n: number) => (n / 1048576).toFixed(1);

// Live download progress (app-styled), shown while the update downloads.
export function UpdateProgress({bytes, total, paused, error, onCancel}: Props) {
  const insets = useSafeAreaInsets();
  const pct = total > 0 ? Math.min(1, bytes / total) : 0;

  const label = error
    ? 'Ошибка загрузки'
    : paused
    ? 'Пауза — ждём сеть…'
    : total > 0
    ? `${mb(bytes)} из ${mb(total)} МБ`
    : 'Подготовка…';

  return (
    <View style={[s.wrap, {paddingTop: insets.top + 8}]} pointerEvents="box-none">
      <View style={s.card}>
        <View style={s.headerRow}>
          <Text style={s.title}>Обновление</Text>
          <Pressable onPress={onCancel} hitSlop={10}>
            <Text style={s.cancel}>Отмена</Text>
          </Pressable>
        </View>

        <View style={s.track}>
          <View style={[s.fill, {width: `${Math.round(pct * 100)}%`}, error && s.fillError]} />
        </View>

        <View style={s.metaRow}>
          <Text style={s.meta}>{label}</Text>
          {total > 0 && !error ? (
            <Text style={s.meta}>{Math.round(pct * 100)}%</Text>
          ) : null}
        </View>
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  wrap: {position: 'absolute', top: 0, left: 0, right: 0, paddingHorizontal: 12},
  card: {
    backgroundColor: 'rgba(28,28,32,0.96)',
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(255,255,255,0.14)',
    paddingHorizontal: 14,
    paddingVertical: 12,
    gap: 10,
  },
  headerRow: {flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between'},
  title: {color: colors.text, fontSize: 15, fontWeight: '700'},
  cancel: {color: colors.accent, fontSize: 14},
  track: {
    height: 6,
    borderRadius: 3,
    backgroundColor: 'rgba(255,255,255,0.12)',
    overflow: 'hidden',
  },
  fill: {height: '100%', backgroundColor: colors.accent, borderRadius: 3},
  fillError: {backgroundColor: colors.danger},
  metaRow: {flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between'},
  meta: {color: colors.textMuted, fontSize: 13},
});
