/**
 * Ladder Logic Engine - Type definitions
 */

export type ContactType = 'NO' | 'NC';
export type CoilType = 'NORMAL' | 'SET' | 'RESET' | 'NEGATED';
export type ElementType = 'contact' | 'coil' | 'timer' | 'counter' | 'connection';

export interface Position {
  x: number;
  y: number;
}

export interface LadderElement {
  id: string;
  type: ElementType;
  position: Position;
}

export interface Contact extends LadderElement {
  type: 'contact';
  contactType: ContactType;
  variable: string;
}

export interface Coil extends LadderElement {
  type: 'coil';
  coilType: CoilType;
  variable: string;
}

export interface Timer extends LadderElement {
  type: 'timer';
  timerType: 'TON' | 'TOF' | 'TP';
  variable: string;
  preset: number;
}

export interface Counter extends LadderElement {
  type: 'counter';
  counterType: 'CTU' | 'CTD' | 'CTUD';
  variable: string;
  preset: number;
}

export interface Connection extends LadderElement {
  type: 'connection';
  fromElement: string;
  toElement: string;
  points: number[];
}

export interface Rung {
  id: string;
  y: number;
  elements: (Contact | Coil | Timer | Counter)[];
  connections: Connection[];
}

export interface LadderProgram {
  rungs: Rung[];
  variables: Variable[];
  metadata: {
    name: string;
    description: string;
    created?: string;
    modified?: string;
  };
}

export interface Variable {
  name: string;
  type: 'BOOL' | 'INT' | 'REAL' | 'TIME';
  address?: string;
  initialValue?: any;
}
