import Runtime from './default';
import * as API from './origin';
import { PublicEngine } from './local';
type DefaultUse = Runtime;
type NamespaceUse = API.Engine;
type LocalExportUse = PublicEngine;
import { Engine } from './local';
type DirectLocalExport = Engine;
class Local {}
type WrongNamespace = Unknown.Local;
import { Engine as Forwarded } from './forward';
type Forward = Forwarded;
