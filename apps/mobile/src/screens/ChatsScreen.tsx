import React, {useCallback, useState} from 'react';
import {
  View,
  Text,
  FlatList,
  Pressable,
  ActivityIndicator,
  Keyboard,
  useWindowDimensions,
  StyleSheet,
} from 'react-native';
import Ionicons from 'react-native-vector-icons/Ionicons';
import {useNavigation, useFocusEffect} from '@react-navigation/native';
import type {NativeStackNavigationProp} from '@react-navigation/native-stack';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import {SearchBar} from '../components/SearchBar';
import {UserListItem} from '../components/UserListItem';
import {ConversationRow} from '../components/ConversationRow';
import {
  useUserSearch,
  useConversations,
  normalizeQuery,
  ME_ID,
  type PublicUser,
} from '../data/mock';
import type {RootStackParamList} from '../../App';
import {colors} from '../theme';

const SEARCH_H = 54; // collapsed search trigger, hidden above the fold

export function ChatsScreen() {
  const insets = useSafeAreaInsets();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const {height} = useWindowDimensions();
  const me = ME_ID;

  const [active, setActive] = useState(false);
  const [query, setQuery] = useState('');

  const {results, loading: searching, error} = useUserSearch(active ? query : '');
  const {items, loading: loadingConvs, reload} = useConversations();
  const term = normalizeQuery(query);

  useFocusEffect(
    useCallback(() => {
      reload();
    }, [reload]),
  );

  function closeSearch() {
    Keyboard.dismiss();
    setActive(false);
    setQuery('');
  }

  function openChat(userId: string) {
    Keyboard.dismiss();
    navigation.navigate('Chat', {userId});
  }

  return (
    <View style={[s.root, {paddingTop: insets.top}]}>
      <View style={s.header}>
        <Text style={s.title}>Chats</Text>
      </View>

      <FlatList
        data={items}
        keyExtractor={c => c.conversation_id}
        renderItem={({item}) => <ConversationRow item={item} meId={me} onPress={openChat} />}
        contentOffset={{x: 0, y: SEARCH_H}}
        showsVerticalScrollIndicator={false}
        ListHeaderComponent={
          <Pressable style={s.trigger} onPress={() => setActive(true)}>
            <Ionicons name="search" size={18} color={colors.textFaint} />
            <Text style={s.triggerText}>Поиск</Text>
          </Pressable>
        }
        ListEmptyComponent={
          loadingConvs ? (
            <View style={[s.empty, {minHeight: height - insets.top - 160}]}>
              <ActivityIndicator color={colors.textFaint} />
            </View>
          ) : (
            <View style={[s.empty, {minHeight: height - insets.top - 160}]}>
              <Ionicons name="chatbubbles-outline" size={56} color={colors.faint} />
              <Text style={s.emptyText}>Здесь появятся ваши чаты</Text>
            </View>
          )
        }
      />

      {active ? (
        <View style={[s.overlay, {paddingTop: insets.top}]}>
          <SearchBar value={query} onChangeText={setQuery} onCancel={closeSearch} autoFocus />

          {term.length < 2 ? (
            <View style={s.hint}>
              <Ionicons name="at" size={40} color={colors.faint} />
              <Text style={s.hintText}>Введите имя пользователя для поиска</Text>
            </View>
          ) : searching ? (
            <View style={s.hint}>
              <ActivityIndicator color={colors.textFaint} />
            </View>
          ) : error ? (
            <View style={s.hint}>
              <Text style={s.errorText}>{error}</Text>
            </View>
          ) : results.length === 0 ? (
            <View style={s.hint}>
              <Ionicons name="search" size={40} color={colors.faint} />
              <Text style={s.hintText}>Ничего не найдено</Text>
            </View>
          ) : (
            <FlatList
              data={results}
              keyExtractor={u => u.id}
              renderItem={({item}: {item: PublicUser}) => (
                <UserListItem user={item} onPress={u => openChat(u.id)} />
              )}
              keyboardShouldPersistTaps="handled"
              keyboardDismissMode="on-drag"
              ListHeaderComponent={<Text style={s.sectionHeader}>Пользователи</Text>}
              contentContainerStyle={{paddingBottom: insets.bottom + 20}}
            />
          )}
        </View>
      ) : null}
    </View>
  );
}

const s = StyleSheet.create({
  root: {flex: 1, backgroundColor: colors.bg},
  header: {paddingHorizontal: 16, paddingVertical: 14},
  title: {color: colors.text, fontSize: 22, fontWeight: '700'},

  trigger: {
    height: SEARCH_H,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    marginHorizontal: 16,
    marginBottom: 4,
    paddingHorizontal: 12,
    backgroundColor: colors.surface,
    borderRadius: 12,
  },
  triggerText: {color: colors.textFaint, fontSize: 16},

  empty: {alignItems: 'center', justifyContent: 'center', gap: 12},
  emptyText: {color: colors.textFaint, fontSize: 16},

  overlay: {...StyleSheet.absoluteFillObject, backgroundColor: colors.bg},

  sectionHeader: {
    color: colors.textFaint,
    fontSize: 13,
    fontWeight: '600',
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 6,
    textTransform: 'uppercase',
  },

  hint: {flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingTop: 60},
  hintText: {color: colors.textFaint, fontSize: 15},
  errorText: {color: colors.danger, fontSize: 15},
});
