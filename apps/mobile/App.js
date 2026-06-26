import { StatusBar } from 'expo-status-bar';
import { useEffect, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import * as Updates from 'expo-updates';

export default function App() {
  const [status, setStatus] = useState('Ready');

  async function checkForUpdates() {
    // OTA updates only run in release builds with updates enabled.
    if (__DEV__ || !Updates.isEnabled) {
      setStatus('Updates disabled in this build');
      return;
    }
    try {
      setStatus('Checking for updates…');
      const result = await Updates.checkForUpdateAsync();
      if (result.isAvailable) {
        setStatus('Downloading update…');
        await Updates.fetchUpdateAsync();
        setStatus('Update ready — restarting…');
        await Updates.reloadAsync();
      } else {
        setStatus('Up to date');
      }
    } catch (e) {
      setStatus('Update check failed: ' + (e?.message ?? 'unknown'));
    }
  }

  // Auto-check once on launch (in addition to checkAutomatically in app.json).
  useEffect(() => {
    checkForUpdates();
  }, []);

  const running = Updates.updateId
    ? 'OTA update ' + Updates.updateId.slice(0, 8)
    : 'embedded build';

  return (
    <View style={styles.container}>
      <StatusBar style="light" />
      <Text style={styles.title}>SpiritChat</Text>
      <Text style={styles.subtitle}>P2P · End-to-end encrypted · No cloud</Text>

      <Pressable style={styles.button} onPress={checkForUpdates}>
        <Text style={styles.buttonText}>Check for updates</Text>
      </Pressable>
      <Text style={styles.status}>{status}</Text>

      <Text style={styles.meta}>
        runtime {Updates.runtimeVersion ?? '—'} · {running}
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
  button: {
    marginTop: 32,
    backgroundColor: '#0a84ff',
    paddingHorizontal: 24,
    paddingVertical: 12,
    borderRadius: 12,
  },
  buttonText: {
    color: '#ffffff',
    fontSize: 16,
    fontWeight: '600',
  },
  status: {
    color: '#8e8e93',
    fontSize: 14,
    marginTop: 16,
  },
  meta: {
    color: '#48484a',
    fontSize: 12,
    marginTop: 24,
  },
});
