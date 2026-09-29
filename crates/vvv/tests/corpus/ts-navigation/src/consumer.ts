import { Motor as Runtime } from './barrel';
import { Model } from './origin';
import { Hidden } from './origin';
type App = Runtime;
type Missing = Engine;
type Private = Hidden;
function generic<Runtime>(value: Runtime): Runtime { return value; }
interface Box { value: Runtime }
import { DefaultEngine } from './default';
type NotNamed = DefaultEngine;
