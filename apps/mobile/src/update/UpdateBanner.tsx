import React from 'react';
import {View, Text, Pressable, StyleSheet} from 'react-native';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {colors} from '../theme';

interface Props {
  version: string;
  onUpdate: () => void;
  onDismiss: () => void;
}

// "Update available" banner, styled like the rest of the app.
export function UpdateBanner({version, onUpdate, onDismiss}: Props) {
  const insets = useSafeAreaInsets();
  return (
    <View style={[s.wrap, {paddingTop: insets.top + 8}]} pointerEvents="box-none">
      <View style={s.card}>
        <Ionicons name="arrow-down-circle" size={22} color={colors.accent} />
        <View style={s.text}>
          <Text style={s.title}>Доступно обновление</Text>
          <Text style={s.sub}>Версия {version.replace(/^v/i, '')}</Text>
        </View>
        <Pressable style={s.button} onPress={onUpdate} hitSlop={6}>
          <Text style={s.buttonText}>Обновить</Text>
        </Pressable>
        <Pressable onPress={onDismiss} hitSlop={10}>
          <Ionicons name="close" size={20} color={colors.textMuted} />
        </Pressable>
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  wrap: {position: 'absolute', top: 0, left: 0, right: 0, paddingHorizontal: 12},
  card: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    backgroundColor: 'rgba(28,28,32,0.96)',
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(255,255,255,0.14)',
    paddingHorizontal: 14,
    paddingVertical: 12,
  },
  text: {flex: 1},
  title: {color: colors.text, fontSize: 15, fontWeight: '700'},
  sub: {color: colors.textMuted, fontSize: 13, marginTop: 1},
  button: {
    backgroundColor: colors.accent,
    borderRadius: 10,
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
  buttonText: {color: '#ffffff', fontSize: 14, fontWeight: '600'},
});
