import React from 'react';
import {View, Text, StyleSheet} from 'react-native';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {colors} from '../theme';

export function Placeholder({title, icon}: {title: string; icon: string}) {
  const insets = useSafeAreaInsets();
  return (
    <View style={[s.root, {paddingTop: insets.top}]}>
      <View style={s.header}>
        <Text style={s.title}>{title}</Text>
      </View>
      <View style={s.center}>
        <Ionicons name={icon} size={56} color={colors.faint} />
        <Text style={s.note}>Скоро</Text>
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  root: {flex: 1, backgroundColor: colors.bg},
  header: {paddingHorizontal: 16, paddingVertical: 14},
  title: {color: colors.text, fontSize: 22, fontWeight: '700'},
  center: {flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12},
  note: {color: colors.textFaint, fontSize: 16},
});
