// Entry for the public copy of the landing page (`npm run build:site`): the
// same page the app shows first, on its own. With no Renkeu server behind
// it, the engine check fails and the page offers "Get Renkeu" instead of
// "Open editor".
import { mount } from 'svelte';
import Landing from './landing/Landing.svelte';

mount(Landing, { target: document.getElementById('app') });
