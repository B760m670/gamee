/**
 * SpiritChat — bare React Native (no Expo).
 * Decentralized P2P, end-to-end encrypted messenger.
 *
 * @format
 */

import React from 'react';
import {SafeAreaView, StatusBar, StyleSheet, Text, View} from 'react-native';

function App(): React.JSX.Element {
  return (
    <SafeAreaView style={styles.container}>
      <StatusBar barStyle="light-content" backgroundColor="#000000" />
      <View style={styles.content}>
        <Text style={styles.title}>SpiritChat</Text>
        <Text style={styles.subtitle}>P2P · End-to-end encrypted · No cloud</Text>
        <Text style={styles.note}>
          Bare React Native — no Expo. Porting the old frontend UI and the
          native P2P core comes next.
        </Text>
      </View>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#000000',
  },
  content: {
    flex: 1,
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

export default App;
