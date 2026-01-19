# registrobr-finder

Ferramenta de linha de comando para verificar disponibilidade de domínios `.br` usando a API do Registro.br.

Escrito em Rust para máxima performance, com suporte a requisições paralelas assíncronas.

## Requisitos

- [mise](https://mise.jdx.dev/) (para gerenciar a versão do Rust)
- Ou Rust 1.92.0+ instalado manualmente

## Instalação

```bash
# Clone o repositório
git clone <repo-url>
cd registrobr-finder

# Instale as dependências e compile
mise install
mise exec -- cargo build --release

# O binário estará em ./target/release/registrobr-finder
```

## Uso

```bash
./target/release/registrobr-finder [OPTIONS]
```

### Opções

| Opção | Descrição | Padrão |
|-------|-----------|--------|
| `-d, --digits <N>` | Número de caracteres a gerar (2 ou 3) | 2 |
| `-w, --workers <N>` | Número de requisições paralelas | 20 |
| `-t, --timeout <S>` | Timeout por requisição em segundos | 10 |
| `-s, --suffix <S>` | Sufixo do domínio | .com.br |
| `--letters` | Gerar apenas combinações de letras (a-z) | false |
| `--numbers` | Gerar apenas combinações de números (0-9) | false |
| `-o, --output <FILE>` | Arquivo para salvar domínios disponíveis | - |
| `-c, --check <DOMAINS>` | Verificar domínio(s) específico(s), separados por vírgula | - |
| `-v, --verbose` | Mostra todos os domínios verificados | false |
| `-h, --help` | Exibe ajuda | - |

## Exemplos

### Buscar domínios de 2 caracteres

```bash
# Alfanuméricos (a-z, 0-9) - 1.296 combinações
./target/release/registrobr-finder -d 2

# Apenas letras - 676 combinações
./target/release/registrobr-finder -d 2 --letters

# Apenas números - 100 combinações
./target/release/registrobr-finder -d 2 --numbers
```

### Buscar domínios de 3 caracteres

```bash
# Alfanuméricos - 46.656 combinações
./target/release/registrobr-finder -d 3

# Apenas letras - 17.576 combinações
./target/release/registrobr-finder -d 3 --letters

# Apenas números - 1.000 combinações
./target/release/registrobr-finder -d 3 --numbers
```

### Verificar domínios específicos

```bash
./target/release/registrobr-finder --check "meudominio,outrodominio,teste123"
```

### Usar outro sufixo

```bash
# Verificar domínios .net.br
./target/release/registrobr-finder -d 2 --suffix .net.br

# Verificar domínios .dev.br
./target/release/registrobr-finder -d 3 --letters --suffix .dev.br

# Verificar domínios .org.br
./target/release/registrobr-finder -d 2 --suffix .org.br
```

### Salvar resultados em arquivo

```bash
./target/release/registrobr-finder -d 3 --letters -o disponiveis.txt
```

### Ajustar performance

```bash
# Mais workers = mais rápido (cuidado com rate limiting)
./target/release/registrobr-finder -d 3 -w 50

# Menos workers = mais lento, mas mais seguro
./target/release/registrobr-finder -d 3 -w 5

# Aumentar timeout para conexões lentas
./target/release/registrobr-finder -d 2 -t 30
```

### Modo verbose

```bash
# Exibe status de todos os domínios (não apenas os disponíveis)
./target/release/registrobr-finder -d 2 --numbers -v
```

## Quantidade de combinações

| Caracteres | Tipo | Quantidade |
|------------|------|------------|
| 2 | Alfanumérico (a-z, 0-9) | 1.296 |
| 2 | Apenas letras (a-z) | 676 |
| 2 | Apenas números (0-9) | 100 |
| 3 | Alfanumérico (a-z, 0-9) | 46.656 |
| 3 | Apenas letras (a-z) | 17.576 |
| 3 | Apenas números (0-9) | 1.000 |

## Como funciona

1. O programa gera todas as combinações possíveis de caracteres com o tamanho especificado
2. Para cada combinação, faz uma requisição à API de disponibilidade do Registro.br
3. A API retorna um status indicando:
   - `0` = domínio **disponível**
   - `2` = domínio **registrado** (inclui data de expiração)
   - `3` = domínio **em processo**
   - `4` = domínio **indisponível**
4. Os resultados são exibidos em tempo real com uma barra de progresso

## Rate Limiting

O Registro.br pode aplicar rate limiting se você fizer muitas requisições em pouco tempo. Se você receber muitos erros de "rate limited":

- Reduza o número de workers (`-w 5`)
- Aguarde alguns minutos antes de tentar novamente

## Licença

MIT - veja [LICENSE](LICENSE) para detalhes.
