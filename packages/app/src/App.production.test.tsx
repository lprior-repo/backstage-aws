import { render, screen } from '@testing-library/react';
import App from './App';

it('requires GitHub sign-in without guest access in production', async () => {
  const originalEnv = process.env;
  try {
    process.env = {
      ...originalEnv,
      NODE_ENV: 'test',
      APP_CONFIG: [
        {
          data: {
            app: { title: 'Test' },
            backend: { baseUrl: 'http://localhost:7007' },
            auth: { environment: 'production' },
            techdocs: {
              storageUrl: 'http://localhost:7007/api/techdocs/static/docs',
            },
          },
          context: 'test',
        },
      ] as any,
    };
    render(App.createRoot());
    expect(await screen.findByText('GitHub')).toBeInTheDocument();
    expect(screen.queryByText('Guest')).not.toBeInTheDocument();
  } finally {
    process.env = originalEnv;
  }
});
