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
    defaultTheme: 'dark',
    themes: {
      dark: {
        dark: true,
        colors: {
          background: '#0d0d10',
          surface: '#141519',
          'surface-variant': '#1c1d22',
          primary: '#6c8cff',
          'primary-hover': '#8ba4ff',
          secondary: '#9498a3',
          accent: '#6c8cff',
          error: '#f87171',
          info: '#6c8cff',
          success: '#34d399',
          warning: '#f59e0b',
          'on-background': '#eeeff1',
          'on-surface': '#eeeff1',
          'on-primary': '#0f0f11',
          'on-secondary': '#0f0f11',
          'on-success': '#0f0f11',
          'on-warning': '#0f0f11',
          'on-error': '#0f0f11',
          'on-info': '#0f0f11',
        },
      },
      light: {
        dark: false,
        colors: {
          background: '#f5f2ed',
          surface: '#ffffff',
          'surface-variant': '#f0ede8',
          primary: '#5b7ae8',
          'primary-hover': '#4a6be0',
          secondary: '#6b6f7a',
          accent: '#5b7ae8',
          error: '#dc2626',
          info: '#5b7ae8',
          success: '#16a34a',
          warning: '#d97706',
          'on-background': '#1c1d22',
          'on-surface': '#1c1d22',
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
      rounded: 'md',
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
      rounded: 'xl',
    },
    VChip: {
      rounded: 'md',
    },
  },
});

const app = createApp(App);
app.use(vuetify);
app.mount("#app");
