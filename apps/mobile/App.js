import { StatusBar } from 'expo-status-bar';
import { StyleSheet, Text, View } from 'react-native';

export default function App() {
  return (
    <View style={styles.container}>
      <StatusBar style="light" />
      <Text style={styles.title}>SpiritChat</Text>
      <Text style={styles.subtitle}>P2P · End-to-end encrypted · No cloud</Text>
      <Text style={styles.note}>
        Build skeleton. The reused UI and the native P2P core land next.
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#000000',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 24,
  },
  title: {
    color: '#ffffff',
    fontSize: 40,
    fontWeight: '800',
    letterSpacing: 0.5,
  },
  subtitle: {
    color: '#8e8e93',
    fontSize: 15,
    marginTop: 8,
  },
  note: {
    color: '#48484a',
    fontSize: 13,
    marginTop: 24,
    textAlign: 'center',
    lineHeight: 18,
  },
});
