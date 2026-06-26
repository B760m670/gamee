import React from 'react';
import {View, Text, Image} from 'react-native';
import {colors} from '../theme';

interface AvatarProps {
  uri: string | null | undefined;
  size?: number;
  username?: string | null;
}

export function Avatar({uri, size = 40, username}: AvatarProps) {
  const initials = username ? username.slice(0, 2).toUpperCase() : '?';

  if (!uri) {
    return (
      <View
        style={{
          width: size,
          height: size,
          borderRadius: size / 2,
          backgroundColor: colors.avatarBg,
          alignItems: 'center',
          justifyContent: 'center',
        }}>
        <Text style={{color: '#a1a1aa', fontSize: size * 0.35, fontWeight: '600'}}>
          {initials}
        </Text>
      </View>
    );
  }

  return (
    <Image
      source={{uri}}
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        backgroundColor: colors.avatarBg,
      }}
      resizeMode="cover"
    />
  );
}
