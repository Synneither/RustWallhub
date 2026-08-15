import { createApp } from "vue";
import App from "./App.vue";
import { createVuetify } from 'vuetify';
import "./assets/style.css";

// Vuetify core + 项目内 MDI 图标子集。组件改为按需导入，Vite 只打包用到的组件样式。
import 'vuetify/styles/core';
import './assets/mdi-subset.css';

import {
  VApp,
  VBtn,
  VBtnToggle,
  VCard,
  VCardActions,
  VCardText,
  VCardTitle,
  VChip,
  VCombobox,
  VDataTable,
  VDialog,
  VForm,
  VIcon,
  VList,
  VListItem,
  VListItemTitle,
  VMain,
  VNavigationDrawer,
  VOverlay,
  VPagination,
  VProgressCircular,
  VProgressLinear,
  VSelect,
  VSpacer,
  VSwitch,
  VTab,
  VTabs,
  VTextField,
  VWindow,
  VWindowItem,
} from 'vuetify/components';
import { Ripple } from 'vuetify/directives';

const components = {
  VApp,
  VBtn,
  VBtnToggle,
  VCard,
  VCardActions,
  VCardText,
  VCardTitle,
  VChip,
  VCombobox,
  VDataTable,
  VDialog,
  VForm,
  VIcon,
  VList,
  VListItem,
  VListItemTitle,
  VMain,
  VNavigationDrawer,
  VOverlay,
  VPagination,
  VProgressCircular,
  VProgressLinear,
  VSelect,
  VSpacer,
  VSwitch,
  VTab,
  VTabs,
  VTextField,
  VWindow,
  VWindowItem,
};

const vuetify = createVuetify({
  components,
  directives: { Ripple },
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
          'on-surface-variant': '#e2e4ea',
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
          primary: '#2563eb',
          'primary-hover': '#3b82f6',
          secondary: '#5c6075',
          accent: '#3b82f6',
          error: '#dc2626',
          info: '#3b82f6',
          success: '#16a34a',
          warning: '#d97706',
          'on-background': '#1a1b23',
          'on-surface': '#1a1b23',
          'on-surface-variant': '#1a1b23',
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
