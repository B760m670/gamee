/**
 * SpiritChat — bare React Native (no Expo).
 * Decentralized P2P, end-to-end encrypted messenger.
 *
 * @format
 */

import React from 'react';
import {StatusBar} from 'react-native';
import {NavigationContainer, DefaultTheme} from '@react-navigation/native';
import {createNativeStackNavigator} from '@react-navigation/native-stack';
import {createBottomTabNavigator} from '@react-navigation/bottom-tabs';
import {SafeAreaProvider} from 'react-native-safe-area-context';
import Ionicons from 'react-native-vector-icons/Ionicons';

import {ChatsScreen} from './src/screens/ChatsScreen';
import {ContactsScreen} from './src/screens/ContactsScreen';
import {CallsScreen} from './src/screens/CallsScreen';
import {SettingsScreen} from './src/screens/SettingsScreen';
import {ChatScreen} from './src/screens/ChatScreen';
import {colors} from './src/theme';

export type RootStackParamList = {
  Tabs: undefined;
  Chat: {userId: string};
};

const Stack = createNativeStackNavigator<RootStackParamList>();
const Tab = createBottomTabNavigator();

const navTheme = {
  ...DefaultTheme,
  dark: true,
  colors: {
    ...DefaultTheme.colors,
    background: colors.bg,
    card: colors.bg,
    text: colors.text,
    border: colors.surface,
    primary: colors.accent,
  },
};

const TAB_ICONS: Record<string, [string, string]> = {
  Контакты: ['people-outline', 'people'],
  Звонки: ['call-outline', 'call'],
  Чаты: ['chatbubble-outline', 'chatbubble'],
  Настройки: ['settings-outline', 'settings'],
};

function Tabs() {
  return (
    <Tab.Navigator
      initialRouteName="Чаты"
      screenOptions={({route}) => ({
        headerShown: false,
        tabBarActiveTintColor: colors.accent,
        tabBarInactiveTintColor: colors.textMuted,
        tabBarStyle: {backgroundColor: colors.bg, borderTopColor: colors.surface},
        tabBarIcon: ({color, size, focused}) => {
          const [outline, filled] = TAB_ICONS[route.name] ?? ['ellipse-outline', 'ellipse'];
          return <Ionicons name={focused ? filled : outline} size={size} color={color} />;
        },
      })}>
      <Tab.Screen name="Контакты" component={ContactsScreen} />
      <Tab.Screen name="Звонки" component={CallsScreen} />
      <Tab.Screen name="Чаты" component={ChatsScreen} />
      <Tab.Screen name="Настройки" component={SettingsScreen} />
    </Tab.Navigator>
  );
}

function App(): React.JSX.Element {
  return (
    <SafeAreaProvider>
      <StatusBar barStyle="light-content" backgroundColor={colors.bg} />
      <NavigationContainer theme={navTheme}>
        <Stack.Navigator
          screenOptions={{
            headerStyle: {backgroundColor: colors.bg},
            headerTintColor: colors.text,
            contentStyle: {backgroundColor: colors.bg},
          }}>
          <Stack.Screen name="Tabs" component={Tabs} options={{headerShown: false}} />
          <Stack.Screen name="Chat" component={ChatScreen} options={{title: 'Чат'}} />
        </Stack.Navigator>
      </NavigationContainer>
    </SafeAreaProvider>
  );
}

export default App;
