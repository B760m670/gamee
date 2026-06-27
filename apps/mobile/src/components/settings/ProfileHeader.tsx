import React from 'react';
import {View, Text, StyleSheet} from 'react-native';
import {Avatar} from '../Avatar';
import {colors} from '../../theme';

interface Props {
  name: string;
  subtitle: string;
}

// Profile section at the top of Settings: avatar + name + identity line.
export function ProfileHeader({name, subtitle}: Props) {
  return (
    <View style={s.wrap}>
      <Avatar uri={null} size={100} username={name} />
      <Text style={s.name}>{name}</Text>
      <Text style={s.sub}>{subtitle}</Text>
    </View>
  );
}

const s = StyleSheet.create({
  wrap: {alignItems: 'center', paddingTop: 8, paddingBottom: 4},
  name: {color: colors.text, fontSize: 26, fontWeight: '600', marginTop: 14, textAlign: 'center'},
  sub: {color: colors.textMuted, fontSize: 14, marginTop: 6, marginBottom: 20, textAlign: 'center'},
});
