import React from 'react';
import {Pressable, Text, StyleSheet} from 'react-native';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {colors} from '../theme';

// A single tab button (icon + label). One button = one file.
const ICONS: Record<string, [string, string]> = {
  Контакты: ['people-outline', 'people'],
  Звонки: ['call-outline', 'call'],
  Чаты: ['chatbubble-outline', 'chatbubble'],
  Настройки: ['settings-outline', 'settings'],
};

interface Props {
  name: string;
  focused: boolean;
  onPress: () => void;
}

export function TabBarItem({name, focused, onPress}: Props) {
  const [outline, filled] = ICONS[name] ?? ['ellipse-outline', 'ellipse'];
  const color = focused ? colors.text : colors.textMuted;
  return (
    <Pressable style={s.item} onPress={onPress}>
      <Ionicons name={focused ? filled : outline} size={23} color={color} />
      <Text style={[s.label, {color}]} numberOfLines={1}>
        {name}
      </Text>
    </Pressable>
  );
}

const s = StyleSheet.create({
  item: {flex: 1, alignItems: 'center', justifyContent: 'center', gap: 2, paddingVertical: 6},
  label: {fontSize: 11, fontWeight: '600'},
});
