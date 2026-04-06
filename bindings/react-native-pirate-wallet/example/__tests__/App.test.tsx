/**
 * @format
 */

import 'react-native';
import React from 'react';
import App from '../App';
jest.mock('react-native-pirate-wallet', () => ({
  createPirateWalletSdk: () => ({
    buildInfo: async () => ({
      version: 'test',
      targetTriple: 'test-target',
    }),
    walletRegistryExists: async () => true,
  }),
}));

// Note: import explicitly to use the types shipped with jest.
import {it} from '@jest/globals';

// Note: test renderer must be required after react-native.
import renderer, {act} from 'react-test-renderer';

it('renders correctly', async () => {
  await act(async () => {
    renderer.create(<App />);
    await Promise.resolve();
  });
});
