import { createApp } from '@backstage/frontend-defaults';
import { createFrontendModule } from '@backstage/frontend-plugin-api';
import {
  configApiRef,
  githubAuthApiRef,
  useApi,
} from '@backstage/core-plugin-api';
import {
  SignInPage,
  type IdentityProviders,
} from '@backstage/core-components';
import { SignInPageBlueprint } from '@backstage/plugin-app-react';
import catalogPlugin from '@backstage/plugin-catalog/alpha';
import { navModule } from './modules/nav';

const signIn = SignInPageBlueprint.make({
  params: {
    loader: async () => props => {
      const authProviders = useApi(configApiRef).getOptionalConfig(
        'auth.providers',
      );
      const providers: IdentityProviders = [];
      if (authProviders?.has('guest')) {
        providers.push('guest');
      }
      if (authProviders?.has('github')) {
        providers.push({
          id: 'github',
          title: 'GitHub',
          message: 'Sign in with your registered GitHub account',
          apiRef: githubAuthApiRef,
        });
      }
      return <SignInPage {...props} providers={providers} />;
    },
  },
});

export default createApp({
  features: [
    catalogPlugin,
    navModule,
    createFrontendModule({ pluginId: 'app', extensions: [signIn] }),
  ],
});
