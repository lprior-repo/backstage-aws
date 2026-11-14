import { SignInPage } from '@backstage/core-components';
import {
  configApiRef,
  githubAuthApiRef,
  useApi,
} from '@backstage/core-plugin-api';
import { createFrontendModule } from '@backstage/frontend-plugin-api';
import { SignInPageBlueprint } from '@backstage/plugin-app-react';

export const signInModule = createFrontendModule({
  pluginId: 'app',
  extensions: [
    SignInPageBlueprint.make({
      params: {
        loader: async () => props => {
          const config = useApi(configApiRef);
          if (config.getOptionalString('auth.environment') !== 'production') {
            return <SignInPage {...props} providers={['guest']} />;
          }
          return (
            <SignInPage
              {...props}
              provider={{
                id: 'github-auth-provider',
                title: 'GitHub',
                message: 'Sign in using GitHub',
                apiRef: githubAuthApiRef,
              }}
            />
          );
        },
      },
    }),
  ],
});
