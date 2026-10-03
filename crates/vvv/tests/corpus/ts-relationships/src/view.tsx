import { task as renderTask } from './bridge';
export function view() { renderTask(); return <main/>; }
