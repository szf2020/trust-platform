/**
 * Blockly Code Generation Engine
 * Converts Blockly workspace blocks to IEC 61131-3 Structured Text (ST)
 */

export interface BlockDefinition {
  type: string;
  id: string;
  fields?: Record<string, any>;
  inputs?: Record<string, any>;
  next?: string; // ID of the next block in sequence
  x?: number;
  y?: number;
}

export interface BlocklyWorkspace {
  blocks: {
    languageVersion: number;
    blocks: BlockDefinition[];
  };
  variables?: Array<{
    name: string;
    type: string;
    id: string;
  }>;
  metadata?: {
    name: string;
    description?: string;
    version?: string;
  };
}

export interface GeneratedCode {
  structuredText: string;
  variables: Map<string, string>;
  errors: string[];
}

/**
 * Engine for generating ST code from Blockly workspace
 */
export class BlocklyEngine {
  private variables: Map<string, string> = new Map();
  private errors: string[] = [];
  private indentLevel: number = 0;

  constructor() {}

  /**
   * Generate ST code from Blockly workspace
   */
  generateCode(workspace: BlocklyWorkspace): GeneratedCode {
    this.variables.clear();
    this.errors = [];
    this.indentLevel = 0;

    // Process variables
    if (workspace.variables) {
      for (const variable of workspace.variables) {
        this.variables.set(variable.name, variable.type || "BOOL");
      }
    }

    // Generate variable declarations
    const varDeclarations = this.generateVariableDeclarations();

    // Generate program body
    const bodyLines: string[] = [];
    
    if (workspace.blocks && workspace.blocks.blocks) {
      for (const block of workspace.blocks.blocks) {
        const blockCode = this.generateBlockCode(block);
        if (blockCode) {
          bodyLines.push(blockCode);
        }
      }
    }

    // Combine into complete ST program
    const programName = workspace.metadata?.name || "BlocklyProgram";
    const structuredText = this.assembleProgram(programName, varDeclarations, bodyLines);

    return {
      structuredText,
      variables: this.variables,
      errors: this.errors,
    };
  }

  /**
   * Generate variable declarations section
   */
  private generateVariableDeclarations(): string {
    if (this.variables.size === 0) {
      return "";
    }

    const lines: string[] = ["VAR"];
    for (const [name, type] of this.variables) {
      lines.push(`  ${name} : ${type};`);
    }
    lines.push("END_VAR");
    
    return lines.join("\n");
  }

  /**
   * Generate code for a single block
   */
  private generateBlockCode(block: BlockDefinition): string {
    switch (block.type) {
      case "controls_if":
        return this.generateIfBlock(block);
      case "logic_compare":
        return this.generateCompareBlock(block);
      case "math_arithmetic":
        return this.generateArithmeticBlock(block);
      case "variables_set":
        return this.generateSetVariableBlock(block);
      case "io_digital_write":
        return this.generateDigitalWriteBlock(block);
      case "io_digital_read":
        return this.generateDigitalReadBlock(block);
      case "logic_boolean":
        return this.generateBooleanBlock(block);
      case "text":
        return this.generateTextBlock(block);
      case "math_number":
        return this.generateNumberBlock(block);
      default:
        this.errors.push(`Unknown block type: ${block.type}`);
        return `(* Unknown block: ${block.type} *)`;
    }
  }

  /**
   * Generate IF statement
   */
  private generateIfBlock(block: BlockDefinition): string {
    const condition = block.inputs?.["IF"]?.block 
      ? this.generateBlockCode(block.inputs["IF"].block)
      : "FALSE";
    
    const doStatements = block.inputs?.["DO"]?.block
      ? this.generateBlockCode(block.inputs["DO"].block)
      : "";

    this.indentLevel++;
    const indent = "  ".repeat(this.indentLevel);
    const statements = doStatements ? `\n${indent}${doStatements}` : "";
    this.indentLevel--;

    return `IF ${condition} THEN${statements}\nEND_IF;`;
  }

  /**
   * Generate comparison operation
   */
  private generateCompareBlock(block: BlockDefinition): string {
    const op = block.fields?.["OP"] || "EQ";
    const left = block.inputs?.["A"]?.block 
      ? this.generateBlockCode(block.inputs["A"].block)
      : "0";
    const right = block.inputs?.["B"]?.block
      ? this.generateBlockCode(block.inputs["B"].block)
      : "0";

    const opMap: Record<string, string> = {
      "EQ": "=",
      "NEQ": "<>",
      "LT": "<",
      "LTE": "<=",
      "GT": ">",
      "GTE": ">=",
    };

    return `(${left} ${opMap[op] || "="} ${right})`;
  }

  /**
   * Generate arithmetic operation
   */
  private generateArithmeticBlock(block: BlockDefinition): string {
    const op = block.fields?.["OP"] || "ADD";
    const left = block.inputs?.["A"]?.block
      ? this.generateBlockCode(block.inputs["A"].block)
      : "0";
    const right = block.inputs?.["B"]?.block
      ? this.generateBlockCode(block.inputs["B"].block)
      : "0";

    const opMap: Record<string, string> = {
      "ADD": "+",
      "MINUS": "-",
      "MULTIPLY": "*",
      "DIVIDE": "/",
      "POWER": "**",
    };

    return `(${left} ${opMap[op] || "+"} ${right})`;
  }

  /**
   * Generate variable assignment
   */
  private generateSetVariableBlock(block: BlockDefinition): string {
    const varName = block.fields?.["VAR"] || "temp";
    const value = block.inputs?.["VALUE"]?.block
      ? this.generateBlockCode(block.inputs["VALUE"].block)
      : "0";

    return `${varName} := ${value};`;
  }

  /**
   * Generate digital output write
   */
  private generateDigitalWriteBlock(block: BlockDefinition): string {
    const address = block.fields?.["ADDRESS"] || "%QX0.0";
    const value = block.inputs?.["VALUE"]?.block
      ? this.generateBlockCode(block.inputs["VALUE"].block)
      : "FALSE";

    return `${address} := ${value};`;
  }

  /**
   * Generate digital input read
   */
  private generateDigitalReadBlock(block: BlockDefinition): string {
    const address = block.fields?.["ADDRESS"] || "%IX0.0";
    return address;
  }

  /**
   * Generate boolean constant
   */
  private generateBooleanBlock(block: BlockDefinition): string {
    const value = block.fields?.["BOOL"] || "TRUE";
    return value;
  }

  /**
   * Generate text constant
   */
  private generateTextBlock(block: BlockDefinition): string {
    const text = block.fields?.["TEXT"] || "";
    return `'${text.replace(/'/g, "''")}'`;
  }

  /**
   * Generate number constant
   */
  private generateNumberBlock(block: BlockDefinition): string {
    const num = block.fields?.["NUM"] || "0";
    return String(num);
  }

  /**
   * Assemble complete ST program
   */
  private assembleProgram(name: string, varDeclarations: string, bodyLines: string[]): string {
    const sections: string[] = [
      `PROGRAM ${name}`,
      "",
    ];

    if (varDeclarations) {
      sections.push(varDeclarations);
      sections.push("");
    }

    sections.push("(* Generated from Blockly *)");
    sections.push("");
    sections.push(...bodyLines);
    sections.push("");
    sections.push("END_PROGRAM");

    return sections.join("\n");
  }
}
