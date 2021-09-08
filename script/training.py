#!/usr/bin/env python

import numpy as np
import tensorflow as tf
from tensorflow import keras
from tensorflow.keras import layers
import os
from xml.etree import ElementTree
import string
import argparse
import time
import random
import tempfile
import re
from decimal import Decimal


# Our source data includes letters, numbers, and punctuation - all ascii
CHARACTERS = " " + string.digits + string.ascii_letters + string.punctuation
# CHARACTERS = string.printable

# We need to reserve an additional class here for the "blank" character
CLASSES = len(CHARACTERS) + 1

CHAR_TO_INDEX = {c: i for i, c in enumerate(CHARACTERS)}
INDEX_TO_CHAR = {i: c for i, c in enumerate(CHARACTERS)}

def encode_string(string):
    return np.array([CHAR_TO_INDEX[c] for c in string], dtype='int32')

def decode_string(string):
    return "".join(INDEX_TO_CHAR[c] for c in string if c != -1)

def load_tensors(test_file):
    with tf.io.gfile.GFile(test_file) as f:
        lines = [t.strip() for t in f.readlines()]

    result = []
    for line in lines:
        text_and_points = line.split("\t")
        assert len(text_and_points) == 2, line
        text = text_and_points[0]
        if ";" in text_and_points[1]:
            (big_sep, small_sep) = (";", ",")
        else:
            (big_sep, small_sep) = (",", " ")

        points = [[float(n) for n in p.split(small_sep)] for p in text_and_points[1].split(big_sep)]
        result.append((text, np.array(points, dtype='float32')))
    return result

def save_tensors(pairs, path):
    with open(path, 'w') as f:
        for (text, points) in pairs:
            f.write(text)
            f.write("\t")
            f.write(";".join(",".join(f'{a:.3f}' for a in parts) for parts in points))
            f.write("\n")

def load_inks(filename):
    with tf.io.gfile.GFile(filename) as f:
        lines = [t.strip() for t in f.readlines()]

    result = []
    for line in lines:
        text_and_points = line.split("\t")
        assert len(text_and_points) == 2, line
        text = text_and_points[0]
        strokes = [
            np.array([
                [float(n) for n in p.split(" ")]
                for p in s.split(",")
            ], dtype='float32')
            for s in text_and_points[1].split(";")
        ]
        result.append((text, strokes))
    return result

def save_inks(pairs, filename):
    with open(filename, 'w') as f:
        for (text, points) in pairs:
            f.write(text)
            f.write("\t")
            f.write(";".join(",".join(" ".join(f'{a:.3f}' for a in p) for p in s) for s in points))
            f.write("\n")

def levenstein(a, b):
    len_a, len_b = len(a)+1, len(b)+1
    matrix = np.zeros(shape=(len_a, len_b), dtype='int32')
    matrix[:,0] = np.arange(len_a)
    matrix[0,:] = np.arange(len_b)
    for a_i, a_c in zip(range(1, len_a), a):
        for b_i, b_c in zip(range(1, len_b), b):
            if a_c == b_c:
                matrix[a_i, b_i] = matrix[a_i-1, b_i-1]
            else:
                matrix[a_i, b_i] = 1 + min(matrix[a_i-1, b_i-1], matrix[a_i, b_i-1], matrix[a_i-1, b_i])
    return matrix[-1, -1]

def cer(pred, true):
    return levenstein(pred, true) / len(true)

def dataset_from_pairs(pairs):
    """
    Accepts string->[N,4]d tensor pairs and returs a dataset
    """
    pairs = list(pairs)
    lines_tensor = tf.ragged.constant([encode_string(p[0]) for p in pairs])
    inks_tensor = tf.ragged.constant([p[1] for p in pairs], dtype=tf.float32)

    return tf.data.Dataset.from_tensor_slices({
        "lines": lines_tensor.to_tensor(),
        "line_lengths": tf.reshape(lines_tensor.row_lengths(), (-1, 1)),
        "inks": inks_tensor.to_tensor(),
        "ink_lengths": tf.reshape(inks_tensor.row_lengths(), (-1, 1)),
    })

def ctc_decode(predicted_labels, predicted_lengths, beam_width=1):
    decoded, probabilities = keras.backend.ctc_decode(
        predicted_labels,
        tf.squeeze(predicted_lengths, axis=1),
        greedy=(beam_width == 1),
        beam_width=beam_width,
    )
    return [decode_string(l) for l in decoded[0].numpy().tolist()]

def ctc_loss(true_labels, true_lengths, predicted_labels, predicted_lengths):
    # Yes, the order is weird, but correct.
    return keras.backend.ctc_batch_cost(true_labels, predicted_labels, predicted_lengths, true_lengths)

def load_ondb(data_dir, name):
    BAD_DATA = [
        "l07-851z", # Missing or extra words
        "p06-804z",
        "g09-310z",
        "a01-004w",
        "h02-037",
        "a04-077",
        "m01-059z",
        "k10-058z",
        "l05-588z",
        "g06-179z",
        "g07-213z",
        "l06-637z",
        "d09-674z",
        "p09-110z",
        "c04-198z",
        "a08-551z", # A million % characters
    ]

    with open(os.path.join(data_dir, f"{name}.txt")) as f:
        training_set = [t.strip() for t in f.readlines()]

    result = []
    for ascii_file in training_set:
        if ascii_file in BAD_DATA:
            print("Discarding: ", ascii_file)
            continue

        relative_dir = os.path.join(ascii_file[:3], ascii_file[:7])
        ascii_path = os.path.join(data_dir, "ascii", relative_dir, ascii_file + ".txt")

        with open(ascii_path) as f:
            ascii_lines = [t.strip() for t in f.readlines()]

        csr_index = ascii_lines.index("CSR:")
        ascii_lines = ascii_lines[csr_index+2:]

        for i, line in enumerate(ascii_lines):
            xml_path = os.path.join(data_dir, "lineStrokes", relative_dir, f"{ascii_file}-{i+1:02d}.xml")
            xml = ElementTree.parse(xml_path)
            ink = []
            for stroke in xml.findall("./StrokeSet/Stroke"):
                points = []
                for point in stroke.findall("./Point"):
                    x = float(point.attrib['x'])
                    y = float(point.attrib['y'])
                    t = Decimal(point.attrib['time'])
                    points.append(np.asarray([x, y, t]))

                ink.append(points)

            min_t = min(x[2] for stroke in ink for x in stroke)

            # We mostly normalize elsewhere, but `t` can readily overflow a 32-bit float
            for stroke in ink:
                for p in stroke:
                    p[2] = float(p[2] - min_t)

            line = clean_iam_text(line)

            result.append((line, [np.array(s, dtype='float32') for s in ink]))

    return result


def clean_iam_text(line):
    after = (
        line.replace(",,", "\"")
            .replace("`", "'")
            .replace(" ,", ",")
            .replace(" .", ".")
            .replace(" !", "!")
            .replace(" ?", "?")
            .replace(" )", ")")
            .replace("( ", "(")
            .replace(" :", ":")
            .replace("n ' t", "n't")
            .replace(" ' s ", "'s ")
    )
    if after.startswith("\" "):
        after = "\"" + after[2:]
    if after.endswith(" \""):
        after = after[:-2] + "\""
    if after.startswith("' "):
        after = "'" + after[2:]
    if after.endswith(" '"):
        after = after[:-2] + "'"
    if line != after:
        print("Cleaned line:", after)
    return after


def normalize(pairs):
    results = []
    for line, array in pairs:
        ink = list(array)

        # fix up time axis: remove reversals or big jumps
        last_time = ink[0][3]
        pauses = 0.0
        for i in ink:
            current_time = i[2]

            # correct for pauses
            current_time = current_time - pauses

            # expect time to be monotonic!
            current_time = max(current_time, last_time)

            # if paused for over half a second, subtract that out and add to the pause counter
            max_time = last_time + 0.5
            if max_time > current_time:
                pauses = pauses + (current_time - max_time)
                current_time = max_time

            i[2] = last_time = current_time

        # normalize ink
        min_x = min([i[0] for i in ink])
        max_x = max([i[0] for i in ink])
        min_y = min([i[1] for i in ink])
        max_y = max([i[1] for i in ink])
        min_t = ink[0][2]
        max_t = ink[-1][2]

        if max_y == min_y:
            print("Skipping: ", line)
            continue

        scale = 1 / (max_y - min_y)
        # Google 2019 normalizes time to path length
        # This seems close enough for English, and easier math!
        time_scale = scale * (max_x - min_x) / (max_t - min_t)

        # perform the scale normalization!
        for i in ink:
            i[0] = (i[0] - min_x) * scale
            i[1] = (i[1] - min_y) * scale
            i[2] = (i[2] - min_t) * time_scale

        # Downsample: ignore points that are close to the previous point
        sampled = [ink[0]]
        for i in ink:
            last = sampled[-1]
            # NB: never skip the last point in the line!
            if i[3] < 0.0 or (last[0] - i[0]) ** 2 + (last[1] - i[1]) ** 2 > 0.0025:
                sampled.append(i)
        ink = sampled

        results.append((line, np.array(ink)))

    return results


def load_docdb(data_dir, name, data_type):
    with open(os.path.join(data_dir, name)) as f:
        files = [t.strip() for t in f.readlines()]

    results = []
    for file in files:
        # 856a is specified in set 4 but doesn't seem to exist
        # 024 and 227 have vertical text in a way we don't care to support
        if file in ['856a.inkml', '024.inkml', '227.inkml']:
            continue
        tree = ElementTree.parse(os.path.join(data_dir, file))

        mapping = tree.find(".//mapping")
        mapping_type = mapping.attrib['type']
        if mapping_type == 'identity':
            transform = np.identity(3)
        elif mapping_type == 'affine':
            transform = np.transpose(np.array([
                [float(f) for f in line.split(" ")]
                for line
                in mapping.find(".//matrix").text.split(",")[:-1]
            ])[:3, :3])
        else:
            print("Unexpected mapping type ", mapping_type, " in file ", file)
            continue

        traces = tree.findall("./trace")

        def parse_trace_string(text):
            # NB: we assume the usual pattern of pos, velocity, accel, accel...
            # because it's really annoying to parse out the actual sigils.
            numbers = [
                [float(n) for n in re.findall(r'-?[0-9]*\.?[0-9]*', line) if n][:3]
                for line in text.split(",")
            ]

            points = []
            arrays = list(np.array(numbers))
            position = arrays[0]
            points.append(position)
            if len(arrays) > 1:
                velocity = arrays[1]
                position = position + velocity
                points.append(position)
                for acceleration in arrays[2:]:
                    velocity = velocity + acceleration
                    position = position + velocity
                    points.append(position)

            array = np.array(points)
            return array.dot(transform)

        id_to_trace = {
            trace.attrib['{http://www.w3.org/XML/1998/namespace}id']: parse_trace_string(trace.text)
            for trace in traces
        }

        def nodes_of_type(root, type):
            results = []
            for view in root.findall(".//traceView"):
                annotation = view.find('./annotation')
                if annotation is None:
                    continue
                if annotation.text == type:
                    results.append(view)
            return results

        wrapper_type, node_type = {
            'lines': ('Textblock', 'Textline'),
            'words': ('Textblock', 'Word'),
            'table': ('Table', 'Textline'),
        }[data_type]

        for textblock in nodes_of_type(tree, wrapper_type):
            for textline in nodes_of_type(textblock, node_type):
                transcription = textline[1].text

                if transcription is None:
                    continue

                transcription = transcription.strip()
                transcription = transcription.replace("´", "'")

                if transcription == "" or "<Symbol/>" in transcription or transcription in " .,-'\"":
                    continue

                if any(c not in CHARACTERS for c in transcription):
                    print(f"Invalid character in `{transcription}`")
                    continue

                traces = [
                    trace.attrib['traceDataRef'][1:]
                    for trace
                    in textline.findall('.//traceView[@traceDataRef]')
                ]
                ink = [id_to_trace[t] for t in traces]

                min_t = ink[0][0, 2]
                for stroke in ink:
                    stroke[:, 2] -= min_t
                assert ink[0][0, 2] == 0.0

                if len(ink) == 0:
                    continue

                results.append((clean_iam_text(transcription), ink))

    return results

import itertools


def is_invalid(text, ink):
    if not ink:
        return "Empty ink"

    last_time = ink[0][0,2]
    for stroke in ink:
        for t in stroke[:, 2]:
            if t < last_time:
                return f"Time goes backwards! {last_time} -> {t}"
            if t > last_time + 8.0:
                return f"Implausibly long wait between samples! {last_time} -> {t}"
            last_time = t

    if ' " ' in text or " ' " in text:
        return f"Suspiciously spaced quote in text: `{text}`"

    if '#' in text:
        return f"Discarding text containing # (used to represent a transcription error): `{text}`"

    return None


def filter_valid(pairs):
    valid = []
    for text, ink in pairs:
        invalid_reason = is_invalid(text, ink)
        if invalid_reason:
            print(invalid_reason)
        else:
            valid.append((text, ink))
    return valid

def augment(pairs, subset, target_size):
    if subset == 'trainset':
        remainders = [0, 1, 2]
    elif subset == 'validset':
        remainders = [3]
    elif subset == 'trainset':
        remainders = [4]
    else:
        remainders = [0, 1, 2, 3, 4]
    pairs = [pair for i, pair in enumerate(pairs) if i % 5 in remainders]

    if not target_size:
        return pairs

    if len(pairs) >= target_size:
        return pairs[:target_size]

    from random import uniform as u

    def transform(strokes):
        j = 0.1
        matrix = np.array([
            [u(1-j, 1+j),   0.0,            0.0],
            [u(-j*2, j*2),  u(1-j, 1+j),    0.0],
            [0.0,           0.0,            u(1-j, 1+j)],
        ])
        return [np.matmul(stroke, matrix) for stroke in strokes]

    output = pairs.copy()
    for line, strokes in itertools.islice(itertools.cycle(pairs), target_size - len(pairs)):
        output.append((line, transform(strokes)))
    return output

def to_deltas(pairs):
    results = []
    for (line, array) in pairs:
        updated = np.copy(array)
        # subtract the point immediately before each point to get the delta
        updated[1:, :3] = array[1:, :3] - array[:-1, :3]
        updated[0, :3] = 0.0
        results.append((line, updated))
    return results


class CTCLayer(layers.Layer):
    def __init__(self, name=None):
        super().__init__(name=name)

    def call(self, true_labels, true_lengths, predicted_labels, predicted_lengths):
        loss = ctc_loss(true_labels, true_lengths, predicted_labels, predicted_lengths)
        self.add_loss(loss)
        return predicted_labels


def build_model(step_size=10):
    MERGE_MODE = 'concat'
    DROPOUT = 0.5
    RNN_SIZE = 64
    RNN_LAYERS = 5

    input_inks = layers.Input(name="inks", shape=(None, step_size))
    input_lines = layers.Input(name='lines', shape=(None,))
    input_ink_lengths = layers.Input(name="ink_lengths", shape=(1,))
    input_line_lengths = layers.Input(name='line_lengths', shape=(1,))

    bidi = input_inks # layers.Masking(name="masking")(input_inks)
    for i in range(RNN_LAYERS):
        lstm = layers.LSTM(RNN_SIZE, return_sequences=True, dropout=DROPOUT)
        bidi = layers.Bidirectional(lstm, merge_mode=MERGE_MODE, name=f"bidi_{i}")(bidi)

    softmax = layers.TimeDistributed(layers.Dense(CLASSES, activation="softmax"), name="softmax")(bidi)

    loss_out = CTCLayer(name="ctc")(input_lines, input_line_lengths, softmax, input_ink_lengths)

    model = keras.models.Model(
        inputs={
            "inks": input_inks,
            "lines": input_lines,
            "ink_lengths": input_ink_lengths,
            "line_lengths": input_line_lengths
        },
        outputs=loss_out,
    )

    model.summary()

    return model

def model_to_prediction_model(model):
    prediction_model = keras.models.Model(
        model.get_layer(name="inks").input, model.get_layer(name="softmax").output
    )
    return prediction_model

def save_prediction_model(model, path, steps=4000):
    run_model = tf.function(lambda x: model(x))
    concrete_func = run_model.get_concrete_function(
        tf.TensorSpec([1, steps, 4], model.inputs[0].dtype)
    )
    model.save(path, save_format="tf", signatures=concrete_func)

def test_keras(data_path, model_path, weights_only=True, checkpoint=None):
    # There's some training-related state in the checkpoint that doesn't get restored
    if weights_only:
        model = load_model_and_checkpoint(None, model_path)
    else:
        model = load_model_and_checkpoint(model_path, checkpoint)

    test_count = 32

    prediction_model = model_to_prediction_model(model)

    pairs = load_tensors(data_path)
    validset = dataset_from_pairs(pairs)

    total_error = 0
    for samples in validset.batch(test_count):
        predictions = prediction_model.predict(samples["inks"])
        predicted = ctc_decode(predictions, samples["ink_lengths"])
        for (true, _), pred in zip(pairs, predicted):
            e = cer(pred, true)
            total_error += e
            print(f"{true} -> {pred} [{e:.4}]")
        pairs = pairs[test_count:]
    print(f"Mean CER: {total_error / float(len(validset))}")


def test_tflite(tflite_path, test_file):
    validset = load_tensors(test_file)

    steps = 2014

    interpreter = tf.lite.Interpreter(model_path=tflite_path)

    input_details = interpreter.get_input_details()
    input_index = input_details[0]['index']

    output_details = interpreter.get_output_details()
    output_index = output_details[0]['index']

    step_count = input_details[0]['shape'][1]
    if step_count == 1:
        interpreter.resize_tensor_input(input_index, [1, steps, 4], strict=True)
        step_count = steps

    total_error = 0.0
    total_count = 0

    interpreter.allocate_tensors()

    for (line, points) in validset:
        if len(points) > step_count:
            # should truncate, but it's not working for some reason
            continue

        ink_length = len(points)
        before = time.monotonic()
        interpreter.reset_all_variables()
        padding = step_count - points.shape[0]
        if padding >= 0:
            input_array = np.pad(points, ((0, padding), (0, 0)))
        else:
            input_array = points[:step_count, :]

        print(input_array.shape, points.shape, line)
        interpreter.set_tensor(input_index, np.array([input_array]))
        interpreter.invoke()

        output_data = interpreter.get_tensor(output_index)
        predicted = ctc_decode(tf.constant(output_data), np.array([[ink_length]]), beam_width=1)
        after = time.monotonic()

        my_cer = cer(predicted[0], line)
        total_error += my_cer
        total_count += 1
        for pred in predicted:
            print(f"{after-before:.3f}s {my_cer:.3f}cer {pred}")

    print(f"Mean CER: {total_error / total_count}")


def validate(pairs):

    for (i, (line, array)) in enumerate(pairs):

        errors = []
        if len(line) * 2 >= len(array):
            errors.append(f"too short [{len(line)} -> {len(array)}]")

        if any(abs(x) > 30 for x in array[:, 0]):
            errors.append("x out of range")

        if any(abs(y) > 2 for y in array[:, 1]):
            errors.append("y out of range")

        if any(abs(x) > 30 for x in array[:, 2]):
            errors.append("t out of range")

        if array[0, 3] < 0.5 or array[-1, 3] > -0.5:
            errors.append("missing pen marks")

        if errors:
            print(f"Bad line {i}: {','.join(errors)}. Text: `{line}`")


def keras_to_tflite(model, checkpoint, tflite_path, dataset, weights_only, steps):
    model = load_model_and_checkpoint(model, checkpoint)

    prediction_model = model_to_prediction_model(model)
    with tempfile.TemporaryDirectory(prefix="prediction-model") as tmp:
        save_prediction_model(prediction_model, tmp, steps)
        # prediction_model.save(tmp)
        converter = tf.lite.TFLiteConverter.from_saved_model(tmp)
        # converter._experimental_lower_tensor_list_ops = False
        # converter.target_spec.supported_ops = [
        #     tf.lite.OpsSet.TFLITE_BUILTINS, tf.lite.OpsSet.SELECT_TF_OPS
        # ]
        tflite_model = converter.convert()

    print("Generated model", len(tflite_model))

    with open(tflite_path, 'wb') as f:
        f.write(tflite_model)

def load_model_and_checkpoint(model, checkpoint):
    if model:
        print('Loading model...')
        model = keras.models.load_model(model)
    else:
        print("Building model...")
        model = build_model()

    if checkpoint and tf.io.gfile.exists(f"{checkpoint}.index"):
        print("Loading existing weights...")
        model.load_weights(checkpoint).expect_partial()

    return model

if __name__ == '__main__':
    parser = argparse.ArgumentParser(prog='training.py')
    subparsers = parser.add_subparsers()

    def ondb_to_text(ondb_path, text_path, subset):
        save_inks(filter_valid(load_ondb(ondb_path, subset)), text_path)

    subcommand = subparsers.add_parser("ondb_to_text")
    subcommand.add_argument("ondb_path", type=str)
    subcommand.add_argument("text_path", type=str)
    subcommand.add_argument("--subset", type=str, default="trainset")
    subcommand.set_defaults(func=ondb_to_text)

    def docdb_to_text(docdb_path, text_path, subset, data_type):
        save_inks(filter_valid(load_docdb(docdb_path, subset, data_type)), text_path)

    subcommand = subparsers.add_parser("docdb_to_text")
    subcommand.add_argument("docdb_path", type=str)
    subcommand.add_argument("text_path", type=str)
    subcommand.add_argument("--subset", type=str, default="0.txt")
    subcommand.add_argument("--data_type", type=str, default='lines')
    subcommand.set_defaults(func=docdb_to_text)

    def augment_text(from_path, to_path, subset, target_size):
        save_inks(augment(load_inks(from_path), subset, target_size), to_path)

    subcommand = subparsers.add_parser("augment")
    subcommand.add_argument("from_path", type=str)
    subcommand.add_argument("to_path", type=str)
    subcommand.add_argument("--subset", type=str, default=None)
    subcommand.add_argument("--target_size", type=int, default=None)
    subcommand.set_defaults(func=augment_text)

    def validate_text(path):
        validate(load_tensors(path))

    subcommand = subparsers.add_parser("validate")
    subcommand.add_argument("path", type=str)
    subcommand.set_defaults(func=validate_text)

    def normalize_text(from_path, to_path):
        save_tensors(normalize(load_tensors(from_path)), to_path)

    subcommand = subparsers.add_parser("normalize")
    subcommand.add_argument("from_path", type=str)
    subcommand.add_argument("to_path", type=str)
    subcommand.set_defaults(func=normalize_text)

    def text_to_deltas(from_path, to_path):
        save_tensors(to_deltas(load_tensors(from_path)), to_path)

    subcommand = subparsers.add_parser("to_delta")
    subcommand.add_argument("from_path", type=str)
    subcommand.add_argument("to_path", type=str)
    subcommand.set_defaults(func=text_to_deltas)

    def do_train(root, trainset, validset, model, checkpoint):
        if root:
            trainset = os.path.join(root, "trainset.txt")
            validset = os.path.join(root, "validset.txt")
            model = os.path.join(root, "model")
            checkpoint = os.path.join(root, "checkpoint")

        model = load_model_and_checkpoint(model, checkpoint)

        print("Fetching data...")
        training_data = load_tensors(trainset)
        validation_data = load_tensors(validset)

        def dataset_from_pairs(pairs):
            """
            Accepts string->[N,4]d tensor pairs and returns a dataset
            """
            import random
            random.shuffle(pairs)

            cleaned = []
            for line, ink in pairs:
                if len(line) * 2 <= len(ink):
                    cleaned.append((line, ink))
                else:
                    print(f"OH NO: ink too short for input `{line}` (ink len {len(ink)})")
            pairs = cleaned

            lines_tensor = tf.ragged.constant([encode_string(p[0]) for p in pairs])
            inks_tensor = tf.ragged.constant([p[1] for p in pairs], dtype=tf.float32)

            def fit_batches(batch):
                max_ink = tf.reduce_max(batch["ink_lengths"])
                batch["inks"] = tf.slice(batch["inks"], [0, 0, 0], [-1, max_ink, -1])
                max_ink = tf.reduce_max(batch["line_lengths"])
                batch["lines"] = tf.slice(batch["lines"], [0, 0], [-1, max_ink])
                return batch

            dataset = tf.data.Dataset.from_tensor_slices({
                "lines": lines_tensor.to_tensor(),
                "line_lengths": tf.reshape(lines_tensor.row_lengths(), (-1, 1)),
                "inks": inks_tensor.to_tensor(),
                "ink_lengths": tf.reshape(inks_tensor.row_lengths(), (-1, 1)),
            })

            return dataset.batch(8).map(fit_batches)

        print("Building trainset...")
        trainset = dataset_from_pairs(training_data)
        print("Building validset...")
        validset = dataset_from_pairs(validation_data)

        callbacks = [
            tf.keras.callbacks.ModelCheckpoint(
                filepath=checkpoint,
                save_weights_only=True,
                save_best_only=True,
                monitor='val_loss'
            ),
            tf.keras.callbacks.TensorBoard(
                log_dir='logs',
                histogram_freq=10,
                write_graph=False,
                write_images=True,
            ),
        ]

        print("Starting to train!")
        model.compile(
            optimizer=keras.optimizers.Adam(
                learning_rate=0.0001,
                clipnorm=9,
            ),
        )

        model.fit(
            trainset.shuffle(1000),
            callbacks=callbacks,
            epochs=1000,
            validation_data=validset,
        )

    subcommand = subparsers.add_parser("train")
    subcommand.add_argument("--root", type=str, default=None)
    subcommand.add_argument("--trainset", type=str, default=None)
    subcommand.add_argument("--validset", type=str, default=None)
    subcommand.add_argument("--model", type=str, default=None)
    subcommand.add_argument("--checkpoint", type=str, default=None)
    subcommand.set_defaults(func=do_train)

    subcommand = subparsers.add_parser("keras_to_tflite")
    subcommand.add_argument("model", type=str)
    subcommand.add_argument("--checkpoint", type=str, default=None)
    subcommand.add_argument("tflite_path", type=str)
    subcommand.add_argument("--dataset", type=str, default=None)
    subcommand.add_argument("--weights_only", default=False, action='store_true')
    subcommand.add_argument("--steps", type=int, default=1024)
    subcommand.set_defaults(func=keras_to_tflite)

    subcommand = subparsers.add_parser("test_keras")
    subcommand.add_argument("data_path", type=str)
    subcommand.add_argument("model_path", type=str)
    subcommand.add_argument("--weights_only", default=False, action='store_true')
    subcommand.add_argument("--checkpoint", type=str, default=None)
    subcommand.set_defaults(func=test_keras)

    subcommand = subparsers.add_parser("test_tflite")
    subcommand.add_argument("tflite_path", type=str)
    subcommand.add_argument("--test_file", type=str, default=None)
    subcommand.set_defaults(func=test_tflite)

    def create_keras(path, step_size):
        model = build_model(step_size=step_size)
        model.save(path)

    subcommand = subparsers.add_parser("create_keras")
    subcommand.add_argument("path", type=str)
    subcommand.add_argument("--step_size", type=int, default=4)
    subcommand.set_defaults(func=create_keras)

    parsed = vars(parser.parse_args())
    command = parsed.pop("func")
    command(**parsed)
