import { render, screen } from '@testing-library/react';
import App from './App';

describe('App', () => {
  const originalEnv = process.env;

  afterEach(() => {
    process.env = originalEnv;
  });

  function renderApp(environment: string) {
    process.env = {
      ...originalEnv,
      NODE_ENV: 'test',
      APP_CONFIG: [
        {
          data: {
            app: { title: 'Test' },
            backend: { baseUrl: 'http://localhost:7007' },
            auth: { environment },
            techdocs: {
              storageUrl: 'http://localhost:7007/api/techdocs/static/docs',
            },
          },
          context: 'test',
        },
      ] as any,
    };

    return render(App.createRoot());
  }

  it('offers guest sign-in for local development', async () => {
    renderApp('development');
    expect(await screen.findByText('Guest')).toBeInTheDocument();
    expect(screen.queryByText('GitHub')).not.toBeInTheDocument();
  });
});
