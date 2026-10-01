#ifndef SUJIU_H
#define SUJIU_H

#ifdef __cplusplus
extern "C" {
#endif

char *sujiu_core_version(void);
char *sujiu_compile_prompt_json(const char *input_json);
void sujiu_string_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
