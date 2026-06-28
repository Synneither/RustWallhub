import { createApp } from "vue";
import App from "./App.vue";
import { createVuetify } from 'vuetify';
import "./assets/style.css";

// Vuetify styles + icons
import 'vuetify/styles';
import '@mdi/font/css/materialdesignicons.css';

import * as components from 'vuetify/components';
import * as directives from 'vuetify/directives';

const vuetify = createVuetify({
  components,
  directives,
  theme: {
    defaultTheme: 'arknights',
    themes: {
      arknights: {
        dark: true,
        colors: {
          background: '#101218',
          surface: '#161827',
          'surface-variant': '#1c1f32',
          primary: '#3b82f6',
          'primary-hover': '#60a5fa',
          secondary: '#8b8fa3',
          accent: '#3b82f6',
          error: '#ef4444',
          info: '#3b82f6',
          success: '#10b981',
          warning: '#f59e0b',
          'on-background': '#e2e4ea',
          'on-surface': '#e2e4ea',
          'on-primary': '#0f1116',
          'on-secondary': '#0f1116',
          'on-success': '#0f1116',
          'on-warning': '#0f1116',
          'on-error': '#0f1116',
          'on-info': '#0f1116',
        },
      },
      light: {
        dark: false,
        colors: {
          background: '#efece6',
          surface: '#faf8f5',
          'surface-variant': '#ffffff',
          primary: '#3b82f6',
          'primary-hover': '#2563eb',
          secondary: '#5c6075',
          accent: '#3b82f6',
          error: '#dc2626',
          info: '#3b82f6',
          success: '#16a34a',
          warning: '#d97706',
          'on-background': '#1a1b23',
          'on-surface': '#1a1b23',
          'on-primary': '#ffffff',
          'on-secondary': '#ffffff',
          'on-success': '#ffffff',
          'on-warning': '#ffffff',
          'on-error': '#ffffff',
          'on-info': '#ffffff',
        },
      },
    },
  },
  defaults: {
    VCard: {
      elevation: 0,
    },
    VBtn: {
      rounded: 'sm',
    },
    VTextField: {
      variant: 'outlined',
      density: 'comfortable',
    },
    VSelect: {
      variant: 'outlined',
      density: 'comfortable',
    },
    VDialog: {
      rounded: 'md',
    },
    VChip: {
      rounded: 'sm',
    },
  },
});

const app = createApp(App);
app.use(vuetify);
app.mount("#app");
