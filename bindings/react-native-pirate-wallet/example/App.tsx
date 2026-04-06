import React, {useEffect, useState} from 'react';
import {SafeAreaView, ScrollView, StyleSheet, Text, View} from 'react-native';
import {createPirateWalletSdk} from 'react-native-pirate-wallet';

function App(): React.JSX.Element {
  const [buildInfo, setBuildInfo] = useState('Loading build info...');
  const [registryState, setRegistryState] = useState(
    'Checking wallet registry...',
  );
  const [lastError, setLastError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    async function runSmoke() {
      try {
        const sdk = createPirateWalletSdk();
        const info = await sdk.buildInfo();
        const registryExists = await sdk.walletRegistryExists();

        if (!active) {
          return;
        }

        setBuildInfo(`build ${info.version} (${info.targetTriple})`);
        setRegistryState(
          `wallet registry exists: ${registryExists ? 'yes' : 'no'}`,
        );
      } catch (error) {
        if (!active) {
          return;
        }

        const message = error instanceof Error ? error.message : String(error);
        setLastError(message);
        setBuildInfo('Native call failed');
        setRegistryState('Wallet registry check failed');
      }
    }

    runSmoke();

    return () => {
      active = false;
    };
  }, []);

  return (
    <SafeAreaView style={styles.root}>
      <ScrollView contentInsetAdjustmentBehavior="automatic" style={styles.root}>
        <View style={styles.card}>
          <Text style={styles.title}>Pirate Wallet RN Example</Text>
          <Text style={styles.subtitle}>
            This app only proves that the local React Native plugin links and
            can call the native backend.
          </Text>

          <View style={styles.row}>
            <Text style={styles.label}>Plugin call</Text>
            <Text style={styles.value}>{buildInfo}</Text>
          </View>

          <View style={styles.row}>
            <Text style={styles.label}>Registry check</Text>
            <Text style={styles.value}>{registryState}</Text>
          </View>

          {lastError ? (
            <View style={styles.errorBox}>
              <Text style={styles.errorTitle}>Native bridge error</Text>
              <Text style={styles.errorText}>{lastError}</Text>
            </View>
          ) : null}
        </View>

        <View style={styles.footer}>
          <Text style={styles.footerText}>
            Keep this example small. Its job is to validate install, linking,
            and one or two native calls.
          </Text>
        </View>
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: '#f6f1e8',
  },
  card: {
    margin: 24,
    padding: 24,
    borderRadius: 20,
    backgroundColor: '#fffaf0',
    borderWidth: 1,
    borderColor: '#d8c8a6',
  },
  title: {
    fontSize: 28,
    fontWeight: '700',
    color: '#2b2012',
  },
  subtitle: {
    marginTop: 8,
    fontSize: 16,
    lineHeight: 22,
    color: '#5a4d3d',
  },
  row: {
    marginTop: 24,
  },
  label: {
    fontSize: 13,
    fontWeight: '700',
    textTransform: 'uppercase',
    letterSpacing: 1,
    color: '#8a6e3b',
  },
  value: {
    marginTop: 6,
    fontSize: 17,
    color: '#2b2012',
  },
  errorBox: {
    marginTop: 24,
    padding: 16,
    borderRadius: 14,
    backgroundColor: '#fff0ea',
    borderWidth: 1,
    borderColor: '#d38b72',
  },
  errorTitle: {
    fontSize: 14,
    fontWeight: '700',
    color: '#8b2f1f',
  },
  errorText: {
    marginTop: 8,
    fontSize: 15,
    lineHeight: 20,
    color: '#8b2f1f',
  },
  footer: {
    paddingHorizontal: 24,
    paddingBottom: 32,
  },
  footerText: {
    fontSize: 14,
    lineHeight: 20,
    color: '#5a4d3d',
  },
});

export default App;
