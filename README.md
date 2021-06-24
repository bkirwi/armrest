# armrest

Armrest is a support library for building applications for the reMarkable tablet.

It currently consists of two main parts:
- A Python machine learning pipeline, used to train a TensorFlow Lite handwriting recognition model.
- A Rust library for high-level application development,
  built on [libremarkable](https://github.com/canselcik/libremarkable),
  and including:
  - A handwriting recognition module, with support for custom language models.
  - An Elm-inspired UI library.

# Building

The rust library is build using `cargo`.
You'll need to set up the build as described in [libremarkable](https://github.com/canselcik/libremarkable).

The `build.rm` wrapper in the project root builds for the tablet.
You'll need the remarkable toolchain downloaded and unpacked somewhere;
set `RM_TOOLCHAIN=<toolchain path>` to point the build script to it.

Building the `tflite` dependency takes a long time...
often several minutes on a reasonably powerful machine.
Sorry about that!
  
# Handwriting recognition

The handwriting recognizer uses a deep LSTM-based architecture, inspired chiefly by the following papers:
- [Fast Multi-language LSTM-based Online Handwriting Recognition](https://arxiv.org/abs/1902.10525)
- [A novel connectionist system for unconstrained handwriting recognition](https://www.cs.toronto.edu/~graves/tpami_2009.pdf)

The full training pipeline is implemented in Python... 
_except_ for the input normalization and encoding,
which is implemented in Rust so it can share code with the runtime HWR.
As a result we need some standard data formats to share the data between Rust and Python.

## Formats

Handwriting data is available in all sorts of formats, many of which are annoying to parse.
`armrest` uses a few simple text-based formats for ease of implementation in multiple languages.

In all cases, records separated by newline characters.
Records are also often accompanied by the text string they correspond to;
in that case, each line includes the string, a tab character, and then the raw data.
(Text strings should not contain tabs... or any whitespace besides the ASCII space.)

### Inks

*Points* are made up of three space-separated decimal values - two for the `x` / `y` position and one for time.
The `y` coordinate grows _downward_.
Points are separated by commas, and *strokes* (sequences of connected points) are separated by semicolons.
A set of strokes that makes up a single logical input is called an *ink*.

### Tensors

In this context, *tensor* is a two-dimensional matrix: a variable-size series of fixed-size steps.
The (decimal) values within a step are separated by spaces; steps are separated by commas.
Types of tensors include:
- `spline` - Each step consists of 4 values: one each for `x`, `y`, and `t`, and a fourth which is 1 iff
  the point is the last in a stroke, and 0 otherwise. `x`, `y`, and `t` are 0 for the first point in an ink,
  and relative to the previous point in the ink for all other points.
- `bezier` - A bezier-curve-based encoding. This is currently experimental, unspecified, and unused.