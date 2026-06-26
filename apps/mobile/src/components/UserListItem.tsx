import React, {memo} from 'react';
import {Pressable, View, Text, StyleSheet} from 'react-native';
import {Avatar} from './Avatar';
import {colors} from '../theme';
import type {PublicUser} from '../data/mock';

interface Props {
  user: PublicUser;
  onPress: (user: PublicUser) => void;
}

function UserListItemBase({user, onPress}: Props) {
  const title =
    user.display_name?.trim() || (user.username ? `@${user.username}` : 'Без имени');
  const subtitle = user.username ? `@${user.username}` : null;

  return (
    <Pressable
      style={({pressed}) => [s.row, pressed && s.rowPressed]}
      onPress={() => onPress(user)}>
      <Avatar uri={user.avatar_url} size={46} username={user.username ?? user.display_name} />
      <View style={s.text}>
        <Text style={s.title} numberOfLines={1}>
          {title}
        </Text>
        {subtitle && title !== subtitle ? (
          <Text style={s.subtitle} numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
    </Pressable>
  );
}

export const UserListItem = memo(UserListItemBase);

const s = StyleSheet.create({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 16,
    paddingVertical: 8,
  },
  rowPressed: {backgroundColor: colors.surfacePressed},
  text: {flex: 1, gap: 2},
  title: {color: colors.text, fontSize: 16, fontWeight: '600'},
  subtitle: {color: colors.textFaint, fontSize: 14},
});
