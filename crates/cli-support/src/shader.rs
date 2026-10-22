//! Text output for one compiled DXBC shader.

use std::io::{self, Write};

use d3dasm::Shader;
use d3dasm::dxbc::chunks::rdef::TypeDesc;

/// Display options of [`write_shader_info`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ShaderInfoOptions {
    /// Show the HLSL type of each constant-buffer variable.
    pub variable_types: bool,
    /// Show only the shader model, compiler and constant buffers. Skip the
    /// resource bindings and the signatures.
    pub constant_buffers_only: bool,
    /// Show the full disassembly.
    pub disassembly: bool,
}

/// Writes the shader model, the resources, the signatures and, as an option,
/// the disassembly of `shader`.
///
/// # Errors
///
/// Returns the I/O error if a write to `out` fails.
pub fn write_shader_info(
    out: &mut impl Write,
    shader: &Shader<'_>,
    options: ShaderInfoOptions,
) -> io::Result<()> {
    if let Some(program) = shader.program() {
        writeln!(
            out,
            "  SM {}.{}",
            program.major_version, program.minor_version
        )?;
    }

    if let Some(resources) = shader.resource_def() {
        if !resources.creator.is_empty() {
            writeln!(out, "  compiler: {}", resources.creator)?;
        }

        for buffer in &resources.constant_buffers {
            writeln!(out, "\n  cbuffer {} ({} bytes):", buffer.name, buffer.size)?;
            for variable in &buffer.variables {
                if options.variable_types {
                    writeln!(
                        out,
                        "    +{:<4} {:<32} {} ({} bytes)",
                        variable.offset,
                        variable.name,
                        format_type(&variable.var_type),
                        variable.size
                    )?;
                } else {
                    writeln!(
                        out,
                        "    +{:<4} {:<32} ({} bytes)",
                        variable.offset, variable.name, variable.size
                    )?;
                }
            }
        }

        if !options.constant_buffers_only && !resources.bindings.is_empty() {
            writeln!(out, "\n  bindings:")?;
            for binding in &resources.bindings {
                writeln!(out, "    {binding}")?;
            }
        }
    }

    if !options.constant_buffers_only {
        if let Some(signature) = shader.input_signature() {
            writeln!(out, "\n  inputs:")?;
            for element in &signature.elements {
                writeln!(out, "    {element}")?;
            }
        }
        if let Some(signature) = shader.output_signature() {
            writeln!(out, "\n  outputs:")?;
            for element in &signature.elements {
                writeln!(out, "    {element}")?;
            }
        }
    }

    if options.disassembly {
        writeln!(out, "\n  --- disassembly ---")?;
        write!(out, "{shader}")?;
    }
    Ok(())
}

/// Formats a resource-definition type as a short HLSL-like name.
///
/// Examples are `float`, `float4` and `floatmat4x4`.
#[must_use]
pub fn format_type(t: &TypeDesc<'_>) -> String {
    let base = match t.var_type {
        0 => "void",
        1 => "bool",
        2 => "int",
        3 => "float",
        4 => "string",
        5 => "texture",
        6 => "texture1d",
        7 => "texture2d",
        8 => "texture3d",
        9 => "texturecube",
        10 => "sampler",
        19 => "uint",
        _ => "?",
    };
    let class = match t.class {
        1 => "vec",     // vector
        2 | 3 => "mat", // row-major or column-major matrix
        _ => "",        // scalar or unknown class
    };
    if t.rows == 1 && t.columns == 1 {
        base.to_string()
    } else if t.rows == 1 {
        format!("{base}{}{columns}", class, columns = t.columns)
    } else {
        format!("{base}{class}{r}x{c}", r = t.rows, c = t.columns)
    }
}

#[cfg(test)]
mod tests;
