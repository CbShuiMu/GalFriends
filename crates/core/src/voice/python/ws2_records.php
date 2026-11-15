<?php
// 只调用固定版本读取器；不执行 WS2 指令。输出保留原生指令边界及字节。
require $argv[1] . '/class_loader.php';
// 固定提交的 Reader 空编码仍走 UTF-16 分支；旧版 WS2 必须按单字节 NUL 终止读取。
class SingleByteReader extends Ws2\Reader {
    public function readString(Helper\FastBuffer &$dataSource): array {
        $start = $dataSource->offset;
        $text = $dataSource->readString();
        return [$text, $dataSource->offset - $start];
    }
}
set_error_handler(function ($severity, $message, $file, $line) {
    if (!(error_reporting() & $severity)) { return; }
    throw new ErrorException($message, 0, $severity, $file, $line);
});
try {
    $bytes = file_get_contents($argv[2]);
    $data = new Helper\FastBuffer(unpack('C*', $bytes));
    $reader = new SingleByteReader('');
    $list = new Ws2\OpcodesList();
    $list->loadList();
    $persistent = new Helper\PersistentContainer();
    $records = [];
    while ($data->count() > 0) {
        $offset = $data->offset;
        $code = $data->shift();
        $class = 'Ws2\\Opcodes\\' . $list->getByOpcode($reader->getHex($code));
        $opcode = new $class($reader, $argv[3] ?? '1.9', $persistent);
        $opcode->decompile($data);
        $length = $data->offset - $offset;
        if ($length != $opcode->getCompiledSize() || $data->offset > strlen($bytes)) {
            throw new Exception('Instruction boundary mismatch at ' . $offset);
        }
        $records[] = ['offset' => $offset, 'code' => $code, 'bytes' => base64_encode(substr($bytes, $offset, $length))];
    }
    echo json_encode($records, JSON_THROW_ON_ERROR);
} catch (Throwable $error) {
    fwrite(STDERR, 'WS2 parse failed at ' . ($offset ?? 0) . ': ' . $error->getMessage() . ' after ' . json_encode(array_map(fn($r) => [$r['offset'], $r['code'], strlen(base64_decode($r['bytes']))], array_slice($records ?? [], -8))));
    exit(1);
}
