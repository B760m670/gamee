import React from 'react';
import {View, Text, StyleSheet} from 'react-native';
import type {RouteProp} from '@react-navigation/native';
import {useRoute} from '@react-navigation/native';
import {MOCK_USERS} from '../data/mock';
import type {RootStackParamList} from '../../App';
import {colors} from '../theme';

export function ChatScreen() {
  const route = useRoute<RouteProp<RootStackParamList, 'Chat'>>();
  const {userId} = route.params;
  const user = MOCK_USERS.find(u => u.id === userId);
  const name = user?.display_name || user?.username || userId;

  return (
    <View style={s.root}>
      <Text style={s.title}>{name}</Text>
      <Text style={s.note}>
        Экран переписки появится на следующем шаге (перенос MessageBubble и
        ChatInputBar), затем подключим P2P-ядро.
      </Text>
    </View>
  );
}

const s = StyleSheet.create({
  root: {flex: 1, backgroundColor: colors.bg, alignItems: 'center', justifyContent: 'center', padding: 24},
  title: {color: colors.text, fontSize: 22, fontWeight: '700'},
  note: {color: colors.textFaint, fontSize: 14, marginTop: 12, textAlign: 'center', lineHeight: 20},
});
