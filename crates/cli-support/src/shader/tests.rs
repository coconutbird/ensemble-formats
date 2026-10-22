use d3dasm::dxbc;
use dxbc::chunks::ChunkWriter;
use dxbc::chunks::rdef::{CBufferDef, CBufferVariable, ResourceBinding, ResourceDef, TypeDesc};
use dxbc::chunks::signature::{SignatureElement, SignatureSystemValue, write_signature};
use dxbc::shex::{Instruction, InstructionKind, Opcode, Program};

use super::{ShaderInfoOptions, format_type, write_shader_info};

fn type_desc(class: u16, var_type: u16, rows: u16, columns: u16) -> TypeDesc<'static> {
    TypeDesc {
        class,
        var_type,
        rows,
        columns,
        elements: 0,
        members: Vec::new(),
        sm5_extra: None,
        name: "".into(),
    }
}

fn variable(
    name: &'static str,
    offset: u32,
    size: u32,
    ty: TypeDesc<'static>,
) -> CBufferVariable<'static> {
    CBufferVariable {
        name: name.into(),
        offset,
        size,
        flags: 0,
        var_type: ty,
        default_value: (&[][..]).into(),
        texture_start: None,
        texture_size: None,
        sampler_start: None,
        sampler_size: None,
    }
}

fn element(name: &'static str, system_value: u32) -> SignatureElement<'static> {
    SignatureElement {
        semantic_name: name.into(),
        semantic_index: 0,
        system_value: SignatureSystemValue::from_u32(system_value),
        component_type: 3,
        register: 0,
        mask: 0xF,
        rw_mask: 0xF,
        padding: 0,
        stream: None,
        min_precision: None,
    }
}

fn program() -> Vec<u8> {
    let ret = Instruction {
        opcode: Opcode::Ret,
        saturate: false,
        test_nonzero: false,
        precise_mask: 0,
        resinfo_return_type: None,
        sync_flags: 0,
        tex_offsets: None,
        resource_dim: None,
        resource_return_type: None,
        kind: InstructionKind::Generic {
            operands: dxbc::shex::Operands::new(),
            trailing_tokens: dxbc::shex::SmallU32Vec::new(),
        },
    };
    dxbc::shex::encode(&Program {
        shader_type: "ps",
        major_version: 4,
        minor_version: 0,
        instructions: vec![ret],
        warnings: Vec::new(),
        fourcc: *b"SHDR",
    })
}

/// Builds an SM 4.0 pixel shader with one constant buffer, one binding,
/// one input and one output.
fn shader_bytes() -> Vec<u8> {
    let resources = ResourceDef {
        constant_buffers: vec![CBufferDef {
            name: "$Globals".into(),
            variables: vec![
                variable("gColor", 0, 16, type_desc(1, 3, 1, 4)),
                variable("gWorld", 16, 64, type_desc(2, 3, 4, 4)),
            ],
            size: 80,
            flags: 0,
            cb_type: 0,
        }],
        bindings: vec![ResourceBinding {
            name: "$Globals".into(),
            input_type: 0,
            return_type: 0,
            dimension: 0,
            num_samples: 0,
            bind_point: 0,
            bind_count: 1,
            flags: 0,
            space: 0,
            id: 0,
        }],
        creator: "test compiler".into(),
        target_version: 0xFFFF_0400,
        compile_flags: 0,
        rd11: None,
        original_data: None,
        original_canonical: None,
    };
    dxbc::build_dxbc(&[
        resources.to_writable(),
        write_signature(*b"ISGN", &[element("TEXCOORD", 0)]),
        write_signature(*b"OSGN", &[element("SV_Target", 64)]),
        dxbc::chunks::WritableChunk {
            fourcc: *b"SHDR",
            data: program(),
        },
    ])
}

fn render(options: ShaderInfoOptions) -> String {
    let bytes = shader_bytes();
    let shaders = d3dasm::parse(&bytes);
    let mut out = Vec::new();
    write_shader_info(&mut out, &shaders[0], options).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn writes_full_info_without_types() {
    let text = render(ShaderInfoOptions::default());
    let expected = concat!(
        "  SM 4.0\n",
        "  compiler: test compiler\n",
        "\n",
        "  cbuffer $Globals (80 bytes):\n",
        "    +0    gColor                           (16 bytes)\n",
        "    +16   gWorld                           (64 bytes)\n",
        "\n",
        "  bindings:\n",
        "    $Globals             cbuffer      NA       0    1    \n",
        "\n",
        "  inputs:\n",
        "    TEXCOORD   float v0.xyzw\n",
        "\n",
        "  outputs:\n",
        "    SV_Target   float v0.xyzw\n",
    );
    assert_eq!(text, expected);
}

#[test]
fn writes_typed_constant_buffers_only() {
    let text = render(ShaderInfoOptions {
        variable_types: true,
        constant_buffers_only: true,
        disassembly: false,
    });
    let expected = concat!(
        "  SM 4.0\n",
        "  compiler: test compiler\n",
        "\n",
        "  cbuffer $Globals (80 bytes):\n",
        "    +0    gColor                           floatvec4 (16 bytes)\n",
        "    +16   gWorld                           floatmat4x4 (64 bytes)\n",
    );
    assert_eq!(text, expected);
}

#[test]
fn appends_disassembly() {
    let bytes = shader_bytes();
    let shaders = d3dasm::parse(&bytes);
    let text = render(ShaderInfoOptions {
        variable_types: false,
        constant_buffers_only: true,
        disassembly: true,
    });
    let expected = format!(
        concat!(
            "  SM 4.0\n",
            "  compiler: test compiler\n",
            "\n",
            "  cbuffer $Globals (80 bytes):\n",
            "    +0    gColor                           (16 bytes)\n",
            "    +16   gWorld                           (64 bytes)\n",
            "\n",
            "  --- disassembly ---\n",
            "{}",
        ),
        shaders[0]
    );
    assert_eq!(text, expected);
}

#[test]
fn formats_scalar_vector_and_matrix_types() {
    assert_eq!(format_type(&type_desc(0, 3, 1, 1)), "float");
    assert_eq!(format_type(&type_desc(1, 19, 1, 3)), "uintvec3");
    assert_eq!(format_type(&type_desc(3, 2, 3, 4)), "intmat3x4");
    assert_eq!(format_type(&type_desc(0, 99, 2, 2)), "?2x2");
}
