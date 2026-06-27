import React from 'react';
import {Pressable, View, StyleSheet, type ViewStyle} from 'react-native';

interface Props {
  onPress: () => void;
  style?: ViewStyle;
  children: React.ReactNode;
}

// Translucent "glass" capsule button — stand-in for the iOS GlassView
// (real blur can be layered in later).
export function GlassButton({onPress, style, children}: Props) {
  return (
    <Pressable onPress={onPress} hitSlop={6}>
      <View style={[s.glass, style]}>{children}</View>
    </Pressable>
  );
}

const s = StyleSheet.create({
  glass: {
    backgroundColor: 'rgba(255,255,255,0.10)',
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(255,255,255,0.18)',
    alignItems: 'center',
    justifyContent: 'center',
  },
});
