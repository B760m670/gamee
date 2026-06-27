import React from 'react';
import {Pressable, StyleSheet, type ViewStyle} from 'react-native';
import {colors} from '../../theme';

interface Props {
  onPress?: () => void;
  style?: ViewStyle | ViewStyle[];
  children: React.ReactNode;
}

// A single tappable settings row (rounded card).
export function SettingsRow({onPress, style, children}: Props) {
  return (
    <Pressable onPress={onPress} style={({pressed}) => [s.row, style, pressed && s.pressed]}>
      {children}
    </Pressable>
  );
}

const s = StyleSheet.create({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 14,
    paddingVertical: 11,
    gap: 12,
    minHeight: 48,
    backgroundColor: colors.surface,
    borderRadius: 16,
    overflow: 'hidden',
  },
  pressed: {backgroundColor: '#3a3a3c'},
});
