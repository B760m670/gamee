import React from 'react';
import {View, TextInput, Pressable, Text, StyleSheet} from 'react-native';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {colors} from '../theme';

interface Props {
  value: string;
  onChangeText: (t: string) => void;
  onCancel: () => void;
  autoFocus?: boolean;
}

export function SearchBar({value, onChangeText, onCancel, autoFocus}: Props) {
  return (
    <View style={s.row}>
      <View style={s.field}>
        <Ionicons name="search" size={18} color={colors.textFaint} />
        <TextInput
          style={s.input}
          value={value}
          onChangeText={onChangeText}
          placeholder="Поиск"
          placeholderTextColor={colors.textFaint}
          autoCapitalize="none"
          autoCorrect={false}
          autoFocus={autoFocus}
          returnKeyType="search"
          accessibilityRole="search"
        />
        {value.length > 0 ? (
          <Pressable onPress={() => onChangeText('')} hitSlop={8}>
            <Ionicons name="close-circle" size={18} color={colors.textFaint} />
          </Pressable>
        ) : null}
      </View>
      <Pressable onPress={onCancel} hitSlop={8}>
        <Text style={s.cancel}>Отмена</Text>
      </Pressable>
    </View>
  );
}

const s = StyleSheet.create({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 16,
    paddingVertical: 8,
  },
  field: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    backgroundColor: colors.surface,
    borderRadius: 12,
    paddingHorizontal: 12,
    height: 38,
  },
  input: {flex: 1, color: colors.text, fontSize: 16, height: '100%', padding: 0},
  cancel: {color: colors.accent, fontSize: 16},
});
