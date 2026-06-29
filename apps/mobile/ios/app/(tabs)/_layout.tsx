import { View, StyleSheet } from 'react-native'
import { NativeTabs } from 'expo-router/unstable-native-tabs'

export const unstable_settings = { initialRouteName: 'messages' }

export default function TabsLayout() {
  return (
    <View style={styles.root}>
      <NativeTabs tintColor="#2f7bff">
        <NativeTabs.Trigger name="contacts" contentStyle={{ backgroundColor: '#000000' }}>
          <NativeTabs.Trigger.Icon sf={{ default: 'person.2', selected: 'person.2.fill' }} />
          <NativeTabs.Trigger.Label>Контакты</NativeTabs.Trigger.Label>
        </NativeTabs.Trigger>

        <NativeTabs.Trigger name="calls" contentStyle={{ backgroundColor: '#000000' }}>
          <NativeTabs.Trigger.Icon sf={{ default: 'phone', selected: 'phone.fill' }} />
          <NativeTabs.Trigger.Label>Звонки</NativeTabs.Trigger.Label>
        </NativeTabs.Trigger>

        <NativeTabs.Trigger name="messages" contentStyle={{ backgroundColor: '#000000' }}>
          <NativeTabs.Trigger.Icon sf={{ default: 'message', selected: 'message.fill' }} />
          <NativeTabs.Trigger.Label>Чаты</NativeTabs.Trigger.Label>
        </NativeTabs.Trigger>

        <NativeTabs.Trigger name="settings" contentStyle={{ backgroundColor: '#000000' }}>
          <NativeTabs.Trigger.Icon sf={{ default: 'gearshape', selected: 'gearshape.fill' }} />
          <NativeTabs.Trigger.Label>Настройки</NativeTabs.Trigger.Label>
        </NativeTabs.Trigger>
      </NativeTabs>
    </View>
  )
}

const styles = StyleSheet.create({ root: { flex: 1, backgroundColor: '#000000' } })
